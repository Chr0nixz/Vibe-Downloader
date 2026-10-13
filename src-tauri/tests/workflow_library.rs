mod common;

use tauri_app_lib::{
    db,
    models::{
        BulkTaskOptionsPatch, SavedView, SiteProfile, TaskPreset, TaskPriority, TaskProxyMode,
        TaskRequestHeaderInput, TaskRequestProfileInput, TaskStatus, WorkflowData, WorkflowEntry,
    },
};

fn view() -> WorkflowEntry {
    WorkflowEntry {
        id: uuid::Uuid::new_v4().to_string(),
        name: "Failed media".into(),
        data: WorkflowData::SavedView(SavedView {
            nav: "issues".into(),
            search: "model".into(),
            sort_key: "created_at".into(),
            sort_direction: "desc".into(),
            file_type: "video".into(),
            source: "all".into(),
            failure: "hls".into(),
            resume: "all".into(),
        }),
    }
}

#[tokio::test]
async fn library_roundtrip_rename_delete_preserves_future_entries() {
    let (_guard, pool) = common::test_pool("workflow-roundtrip").await;
    let mut entry = view();
    db::workflow::save(&pool, &entry).await.expect("save");
    sqlx::query("INSERT INTO workflow_library(id,name,format_version,payload) VALUES('future','future',2,'{}')").execute(&pool).await.expect("future");
    let path: String =
        sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name='main'")
            .fetch_one(&pool)
            .await
            .unwrap();
    pool.close().await;
    let pool = db::connect(std::path::Path::new(&path)).await.unwrap().pool;
    let read = db::workflow::list(&pool).await.expect("read");
    assert_eq!(read.entries.len(), 1);
    assert_eq!(read.unsupported_count, 1);
    assert_eq!(
        serde_json::to_value(&read.entries[0]).unwrap(),
        serde_json::to_value(&entry).unwrap()
    );
    entry.name = "Renamed".into();
    db::workflow::save(&pool, &entry).await.expect("rename");
    assert_eq!(
        db::workflow::list(&pool).await.unwrap().entries[0].name,
        "Renamed"
    );
    db::workflow::delete(&pool, &entry.id).await.unwrap();
    let read = db::workflow::list(&pool).await.unwrap();
    assert!(read.entries.is_empty());
    assert_eq!(read.unsupported_count, 1);
    pool.close().await;
}

#[tokio::test]
async fn site_profiles_reject_secrets_noncanonical_origins_and_unsafe_headers() {
    let (_guard, pool) = common::test_pool("workflow-site").await;
    for name in [
        "Cookie",
        "Authorization",
        "X-Api-Key",
        "Host",
        "Content-Length",
    ] {
        let mut entry = view();
        entry.data = WorkflowData::SiteProfile(SiteProfile {
            origin: "https://example.com".into(),
            profile: TaskRequestProfileInput {
                custom_headers: vec![TaskRequestHeaderInput {
                    name: name.into(),
                    value: "secret".into(),
                }],
                ..Default::default()
            },
        });
        assert!(
            db::workflow::save(&pool, &entry).await.is_err(),
            "must reject {name}"
        );
    }
    let mut entry = view();
    entry.data = WorkflowData::SiteProfile(SiteProfile {
        origin: "https://example.com/path?token=secret".into(),
        profile: TaskRequestProfileInput {
            user_agent: Some("Test".into()),
            ..Default::default()
        },
    });
    assert!(db::workflow::save(&pool, &entry).await.is_err());
    assert!(db::workflow::list(&pool).await.unwrap().entries.is_empty());
    entry.data = WorkflowData::SiteProfile(SiteProfile {
        origin: "https://example.com".into(),
        profile: TaskRequestProfileInput {
            user_agent: Some("Test".into()),
            ..Default::default()
        },
    });
    db::workflow::save(&pool, &entry)
        .await
        .expect("public profile");
}

#[tokio::test]
async fn presets_reject_proxy_credentials_and_extra_secret_fields() {
    let mut entry = view();
    entry.data = WorkflowData::TaskPreset(TaskPreset {
        save_dir: std::env::temp_dir().to_string_lossy().into(),
        category_key: None,
        priority: TaskPriority::Normal,
        task_speed_limit_bps: None,
        obey_schedule: true,
        proxy_mode: TaskProxyMode::Custom,
        proxy_url: Some("socks5://user:secret@localhost:1080".into()),
        proxy_no_proxy: None,
    });
    assert!(db::workflow::validate_entry(&entry).is_err());
    let mut json = serde_json::to_value(&entry).unwrap();
    json["data"]["value"]["password"] = "secret".into();
    assert!(serde_json::from_value::<WorkflowEntry>(json).is_err());
}

#[tokio::test]
async fn batch_patch_100_mixed_tasks_preserves_unselected_fields_and_skips_terminal_or_deleted() {
    let (_guard, pool) = common::test_pool("workflow-batch").await;
    for index in 0..100 {
        let id = format!("batch-{index}");
        let paths = common::TestPaths::new(&id);
        let mut task = common::download_task(
            &id,
            format!("https://example.com/{id}"),
            "https",
            "sample.bin",
            1024,
            &paths,
            true,
        );
        task.status = match index % 4 {
            0 => TaskStatus::Downloading,
            1 => TaskStatus::Paused,
            2 => TaskStatus::Failed,
            _ => TaskStatus::Completed,
        };
        task.task_speed_limit_bps = Some("2048".into());
        task.category_key = Some("original".into());
        db::insert_task_record(&pool, &task).await.unwrap();
    }
    let patch = BulkTaskOptionsPatch {
        priority: Some(TaskPriority::Low),
        ..Default::default()
    };
    for index in 0..100 {
        let id = format!("batch-{index}");
        let applied = db::workflow::patch_task(&pool, &id, &patch).await.unwrap();
        assert_eq!(applied, index % 4 != 3);
        let task = db::get_task_record(&pool, &id).await.unwrap().unwrap();
        assert_eq!(
            task.priority,
            if applied {
                TaskPriority::Low
            } else {
                TaskPriority::Normal
            }
        );
        assert_eq!(task.task_speed_limit_bps.as_deref(), Some("2048"));
        assert_eq!(task.category_key.as_deref(), Some("original"));
        assert!(task.obey_schedule);
    }
    assert!(!db::workflow::patch_task(&pool, "deleted", &patch)
        .await
        .unwrap());
    let patch = BulkTaskOptionsPatch {
        clear_speed_limit: true,
        clear_category: true,
        obey_schedule: Some(false),
        ..Default::default()
    };
    db::workflow::patch_task(&pool, "batch-0", &patch)
        .await
        .unwrap();
    let task = db::get_task_record(&pool, "batch-0")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(task.task_speed_limit_bps, None);
    assert_eq!(task.category_key, None);
    assert!(!task.obey_schedule);
    assert_eq!(task.priority, TaskPriority::Low);
    assert!(db::workflow::validate_patch(&BulkTaskOptionsPatch::default()).is_err());
    assert!(db::workflow::validate_patch(&BulkTaskOptionsPatch {
        clear_speed_limit: true,
        task_speed_limit_bps: Some("100".into()),
        ..Default::default()
    })
    .is_err());
}
