//! PERF-05: task_events retention mirrors request diagnostics (cap + age).

use std::time::{SystemTime, UNIX_EPOCH};

use tauri_app_lib::{
    db,
    models::{
        task::now_iso, HashVerificationStatus, TaskKind, TaskPriority, TaskRecord, TaskStatus,
    },
};

async fn test_pool(label: &str) -> sqlx::SqlitePool {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-db-events-{label}-{id}.sqlite"));
    db::connect(&path).await.expect("connect").pool
}

async fn seed_task(pool: &sqlx::SqlitePool, task_id: &str, status: TaskStatus) {
    let now = now_iso();
    let record = TaskRecord {
        id: task_id.to_string(),
        url: "http://example.com/file".to_string(),
        final_url: Some("http://example.com/file".to_string()),
        protocol: "http".to_string(),
        task_kind: TaskKind::SingleFile,
        file_name: "file.bin".to_string(),
        save_dir: std::env::temp_dir().to_string_lossy().to_string(),
        temp_path: None,
        final_path: None,
        total_size: 100,
        downloaded_bytes: 0,
        status,
        etag: None,
        last_modified: None,
        content_type: Some("application/octet-stream".to_string()),
        supports_resume: true,
        supports_parallel: true,
        supports_multi_file: false,
        source_key: format!("http://example.com/{task_id}"),
        connection_count: 0,
        speed_bps: 0,
        task_speed_limit_bps: None,
        priority: TaskPriority::Normal,
        queue_position: 0,
        category_key: None,
        obey_schedule: true,
        health_summary: None,
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
    };
    db::insert_task_record(pool, &record).await.expect("insert");
}

#[tokio::test]
async fn prune_enforces_per_task_cap() {
    let pool = test_pool("cap").await;
    seed_task(&pool, "t1", TaskStatus::Completed).await;

    let over = db::TASK_EVENTS_MAX_PER_TASK + 25;
    for i in 0..over {
        db::insert_task_event(&pool, "t1", "progress", Some(&format!("n={i}")))
            .await
            .expect("insert event");
    }

    let removed = db::prune_task_events(&pool).await.expect("prune");
    assert!(removed >= 25);

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_events WHERE task_id = 't1'")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(count, db::TASK_EVENTS_MAX_PER_TASK);
}

#[tokio::test]
async fn prune_removes_old_events_but_keeps_latest_pause_for_paused_tasks() {
    let pool = test_pool("age-pause").await;
    seed_task(&pool, "paused-sched", TaskStatus::Paused).await;
    seed_task(&pool, "done", TaskStatus::Completed).await;

    let old = (chrono::Utc::now() - chrono::Duration::days(db::TASK_EVENTS_MAX_AGE_DAYS + 2))
        .to_rfc3339();
    sqlx::query(
        "INSERT INTO task_events (task_id, event_type, payload, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind("paused-sched")
    .bind("paused_by_schedule")
    .bind(Option::<String>::None)
    .bind(&old)
    .execute(&pool)
    .await
    .expect("old pause");
    sqlx::query(
        "INSERT INTO task_events (task_id, event_type, payload, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind("done")
    .bind("completed")
    .bind(Option::<String>::None)
    .bind(&old)
    .execute(&pool)
    .await
    .expect("old completed");

    let removed = db::prune_task_events(&pool).await.expect("prune");
    assert!(removed >= 1);

    let pause = db::get_latest_pause_event_type(&pool, "paused-sched")
        .await
        .expect("latest pause");
    assert_eq!(pause.as_deref(), Some("paused_by_schedule"));

    let done_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM task_events WHERE task_id = 'done'")
            .fetch_one(&pool)
            .await
            .expect("done count");
    assert_eq!(done_count, 0);
}
