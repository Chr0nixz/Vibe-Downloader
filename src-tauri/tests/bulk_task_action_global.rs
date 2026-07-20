//! UX-05: global pause/resume must select targets from the full DB set.

use std::time::{SystemTime, UNIX_EPOCH};

use tauri_app_lib::{
    db,
    models::{HashVerificationStatus, TaskKind, TaskPriority, TaskRecord, TaskStatus},
};

async fn test_pool(label: &str) -> sqlx::SqlitePool {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-bulk-global-{label}-{id}.sqlite"));
    db::connect(&path).await.expect("connect").pool
}

fn sample_task(id: &str, status: TaskStatus, queue_position: i64) -> TaskRecord {
    let now = tauri_app_lib::models::task::now_iso();
    TaskRecord {
        id: id.to_string(),
        url: format!("http://127.0.0.1/{id}"),
        final_url: Some(format!("http://127.0.0.1/{id}")),
        protocol: "http".to_string(),
        task_kind: TaskKind::SingleFile,
        file_name: format!("{id}.bin"),
        save_dir: std::env::temp_dir().to_string_lossy().to_string(),
        temp_path: Some(
            std::env::temp_dir()
                .join(format!("{id}.vibe-downloading"))
                .to_string_lossy()
                .to_string(),
        ),
        final_path: Some(
            std::env::temp_dir()
                .join(format!("{id}.bin"))
                .to_string_lossy()
                .to_string(),
        ),
        total_size: 1024,
        downloaded_bytes: 0,
        status,
        etag: Some("etag-a".to_string()),
        last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".to_string()),
        content_type: Some("application/octet-stream".to_string()),
        supports_resume: true,
        supports_parallel: true,
        supports_multi_file: false,
        source_key: format!("http://127.0.0.1/{id}"),
        connection_count: 0,
        speed_bps: 0,
        task_speed_limit_bps: None,
        priority: TaskPriority::Normal,
        queue_position,
        category_key: None,
        obey_schedule: true,
        health_summary: Some("ok".to_string()),
        error_message: None,
        error_code: None,
        recovery_actions: Vec::new(),
        retry_after_at: None,
        expected_hash_sha256: None,
        actual_hash_sha256: None,
        hash_status: HashVerificationStatus::NotRequested,
        hash_error: None,
        hash_verified_at: None,
        created_at: now.clone(),
        updated_at: now,
        files_version: 0,
    }
}

#[tokio::test]
async fn list_task_ids_by_statuses_covers_full_db_beyond_page_size() {
    let pool = test_pool("page").await;

    // Seed >100 pauseable tasks plus completed noise, mirroring a filtered UI page.
    for i in 0..120 {
        let status = if i < 110 {
            TaskStatus::Queued
        } else {
            TaskStatus::Completed
        };
        db::insert_task_record(&pool, &sample_task(&format!("t-{i:03}"), status, i))
            .await
            .expect("insert");
    }
    for i in 0..5 {
        db::insert_task_record(
            &pool,
            &sample_task(&format!("d-{i}"), TaskStatus::Downloading, 200 + i),
        )
        .await
        .expect("insert downloading");
    }

    let pause_ids = db::list_task_ids_by_statuses(&pool, &["downloading", "retrying", "queued"])
        .await
        .expect("list pause targets");
    assert_eq!(
        pause_ids.len(),
        115,
        "global pause must include every matching row, not a 100-item page"
    );

    let resume_ids = db::list_task_ids_by_statuses(&pool, &["paused", "failed", "waiting_network"])
        .await
        .expect("list resume targets");
    assert!(resume_ids.is_empty());

    db::insert_task_record(&pool, &sample_task("p-1", TaskStatus::Paused, 300))
        .await
        .expect("insert paused");
    db::insert_task_record(&pool, &sample_task("f-1", TaskStatus::Failed, 301))
        .await
        .expect("insert failed");
    db::insert_task_record(&pool, &sample_task("w-1", TaskStatus::WaitingNetwork, 302))
        .await
        .expect("insert waiting");

    let resume_ids = db::list_task_ids_by_statuses(&pool, &["paused", "failed", "waiting_network"])
        .await
        .expect("list resume targets after insert");
    assert_eq!(resume_ids.len(), 3);
}
