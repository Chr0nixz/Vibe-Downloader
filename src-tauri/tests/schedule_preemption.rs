//! FUN-07: disabling schedule downloads resumes only schedule-paused tasks.

mod common;

use tauri_app_lib::{
    commands::tasks::{list_tasks_paused_by_schedule, resume_schedule_paused_tasks},
    db,
    models::{HashVerificationStatus, TaskKind, TaskPriority, TaskRecord, TaskStatus},
};

async fn test_pool(label: &str) -> sqlx::SqlitePool {
    common::test_pool(label).await
}

fn paused_task(id: &str, obey_schedule: bool) -> TaskRecord {
    let now = chrono::Utc::now().to_rfc3339();
    TaskRecord {
        id: id.to_string(),
        url: format!("https://example.com/{id}.bin"),
        final_url: Some(format!("https://example.com/{id}.bin")),
        protocol: "http".to_string(),
        task_kind: TaskKind::SingleFile,
        file_name: format!("{id}.bin"),
        save_dir: std::env::temp_dir().to_string_lossy().to_string(),
        temp_path: None,
        final_path: None,
        total_size: 1024,
        downloaded_bytes: 128,
        status: TaskStatus::Paused,
        etag: None,
        last_modified: None,
        content_type: Some("application/octet-stream".to_string()),
        supports_resume: true,
        supports_parallel: false,
        supports_multi_file: false,
        source_key: "example.com".to_string(),
        connection_count: 0,
        speed_bps: 0,
        task_speed_limit_bps: None,
        priority: TaskPriority::Normal,
        queue_position: 0,
        category_key: None,
        obey_schedule,
        health_summary: Some("Paused".to_string()),
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

async fn seed_paused_with_event(pool: &sqlx::SqlitePool, id: &str, obey: bool, event: &str) {
    db::insert_task_record(pool, &paused_task(id, obey))
        .await
        .expect("insert task");
    // Manual pause always writes "paused" first; schedule auto-pause then
    // appends "paused_by_schedule" so it becomes the latest pause reason.
    db::insert_task_event(pool, id, "paused", None)
        .await
        .expect("paused event");
    if event != "paused" {
        db::insert_task_event(pool, id, event, None)
            .await
            .expect("pause reason event");
    }
}

#[tokio::test]
async fn fun07_disabling_schedule_resumes_schedule_paused_tasks() {
    let pool = test_pool("fun07-resume-schedule").await;
    seed_paused_with_event(&pool, "sched-paused", true, "paused_by_schedule").await;

    let selected = list_tasks_paused_by_schedule(&pool).await.expect("list");
    assert_eq!(selected, vec!["sched-paused".to_string()]);

    let resumed = resume_schedule_paused_tasks(None, &pool, None)
        .await
        .expect("resume");
    assert_eq!(resumed, vec!["sched-paused".to_string()]);

    let task = db::get_task_record(&pool, "sched-paused")
        .await
        .expect("read")
        .expect("exists");
    assert_eq!(task.status, TaskStatus::Queued);
    pool.close().await;
}

#[tokio::test]
async fn fun07_disabling_schedule_skips_manual_pause() {
    let pool = test_pool("fun07-skip-manual").await;
    seed_paused_with_event(&pool, "manual-paused", true, "paused").await;

    let selected = list_tasks_paused_by_schedule(&pool).await.expect("list");
    assert!(selected.is_empty(), "manual pause must not be selected");

    let resumed = resume_schedule_paused_tasks(None, &pool, None)
        .await
        .expect("resume");
    assert!(resumed.is_empty());

    let task = db::get_task_record(&pool, "manual-paused")
        .await
        .expect("read")
        .expect("exists");
    assert_eq!(task.status, TaskStatus::Paused);
    pool.close().await;
}

#[tokio::test]
async fn fun07_schedule_then_manual_pause_not_resumed_on_disable() {
    let pool = test_pool("fun07-override-manual").await;
    seed_paused_with_event(&pool, "then-manual", true, "paused_by_schedule").await;
    // Later manual pause becomes the latest pause reason.
    db::insert_task_event(&pool, "then-manual", "paused", None)
        .await
        .expect("manual override");

    let selected = list_tasks_paused_by_schedule(&pool).await.expect("list");
    assert!(
        selected.is_empty(),
        "latest pause reason is manual, got {selected:?}"
    );

    let resumed = resume_schedule_paused_tasks(None, &pool, None)
        .await
        .expect("resume");
    assert!(resumed.is_empty());
    let task = db::get_task_record(&pool, "then-manual")
        .await
        .expect("read")
        .expect("exists");
    assert_eq!(task.status, TaskStatus::Paused);
    pool.close().await;
}

#[tokio::test]
async fn fun07_disabling_schedule_skips_obey_schedule_false() {
    let pool = test_pool("fun07-obey-false").await;
    seed_paused_with_event(&pool, "no-obey", false, "paused_by_schedule").await;

    let selected = list_tasks_paused_by_schedule(&pool).await.expect("list");
    assert!(
        selected.is_empty(),
        "obey_schedule=false must stay paused, got {selected:?}"
    );

    let resumed = resume_schedule_paused_tasks(None, &pool, None)
        .await
        .expect("resume");
    assert!(resumed.is_empty());
    let task = db::get_task_record(&pool, "no-obey")
        .await
        .expect("read")
        .expect("exists");
    assert_eq!(task.status, TaskStatus::Paused);
    pool.close().await;
}
