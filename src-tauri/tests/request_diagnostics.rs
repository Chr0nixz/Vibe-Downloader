use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::Row;
use tauri_app_lib::{
    db,
    models::{
        task::now_iso, HashVerificationStatus, RequestDiagnosticRecord, TaskKind, TaskPriority,
        TaskRecord, TaskStatus,
    },
};

async fn test_pool(label: &str) -> sqlx::SqlitePool {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-db-{label}-{id}.sqlite"));
    db::connect(&path).await.expect("connect").pool
}

fn sample_record(task_id: &str) -> RequestDiagnosticRecord {
    RequestDiagnosticRecord {
        task_id: task_id.to_string(),
        method: "GET".to_string(),
        url: "http://example.com/file".to_string(),
        range_header: None,
        if_range_header: None,
        status_code: Some(200),
        etag: None,
        last_modified: None,
        content_length: None,
        error_message: None,
        retry_count: 0,
        duration_ms: 10,
    }
}

async fn seed_task(pool: &sqlx::SqlitePool, task_id: &str) {
    let now = now_iso();
    let record = TaskRecord {
        id: task_id.to_string(),
        url: "http://example.com/file".to_string(),
        final_url: Some("http://example.com/file".to_string()),
        protocol: "http".to_string(),
        task_kind: TaskKind::SingleFile,
        file_name: "file.bin".to_string(),
        save_dir: std::env::temp_dir().to_string_lossy().to_string(),
        temp_path: Some(
            std::env::temp_dir()
                .join(format!("{task_id}.vibe-downloading"))
                .to_string_lossy()
                .to_string(),
        ),
        final_path: Some(
            std::env::temp_dir()
                .join(format!("{task_id}.bin"))
                .to_string_lossy()
                .to_string(),
        ),
        total_size: 100,
        downloaded_bytes: 0,
        status: TaskStatus::Queued,
        etag: Some("etag-a".to_string()),
        last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".to_string()),
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
        health_summary: Some("Queued".to_string()),
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
    db::insert_task_record(pool, &record)
        .await
        .expect("insert task");
}

async fn count_for_task(pool: &sqlx::SqlitePool, task_id: &str) -> i64 {
    let row = sqlx::query("SELECT COUNT(*) AS n FROM task_requests WHERE task_id = ?")
        .bind(task_id)
        .fetch_one(pool)
        .await
        .expect("count");
    row.get::<i64, _>("n")
}

async fn count_total(pool: &sqlx::SqlitePool) -> i64 {
    let row = sqlx::query("SELECT COUNT(*) AS n FROM task_requests")
        .fetch_one(pool)
        .await
        .expect("count total");
    row.get::<i64, _>("n")
}

async fn set_all_created_at(pool: &sqlx::SqlitePool, task_id: &str, iso: &str) {
    sqlx::query("UPDATE task_requests SET created_at = ? WHERE task_id = ?")
        .bind(iso)
        .bind(task_id)
        .execute(pool)
        .await
        .expect("update created_at");
}

#[tokio::test]
async fn prune_caps_rows_per_task_to_maximum() {
    let pool = test_pool("prune-cap").await;
    let task_id = "task-cap";
    seed_task(&pool, task_id).await;
    // Insert one more row than the per-task cap.
    let inserted = db::REQUEST_DIAGNOSTICS_MAX_PER_TASK + 10;
    for _ in 0..inserted {
        db::insert_request_diagnostic(&pool, &sample_record(task_id))
            .await
            .expect("insert");
    }
    assert_eq!(count_for_task(&pool, task_id).await, inserted);

    let removed = db::prune_request_diagnostics(&pool).await.expect("prune");

    assert_eq!(
        removed,
        u64::try_from(inserted - db::REQUEST_DIAGNOSTICS_MAX_PER_TASK).unwrap()
    );
    assert_eq!(
        count_for_task(&pool, task_id).await,
        db::REQUEST_DIAGNOSTICS_MAX_PER_TASK
    );
}

#[tokio::test]
async fn prune_removes_rows_older_than_max_age() {
    let pool = test_pool("prune-age").await;
    let task_id = "task-age";
    seed_task(&pool, task_id).await;
    for _ in 0..5 {
        db::insert_request_diagnostic(&pool, &sample_record(task_id))
            .await
            .expect("insert");
    }
    // Force all rows older than the retention window.
    let old_iso = (chrono::Utc::now()
        - chrono::Duration::days(db::REQUEST_DIAGNOSTICS_MAX_AGE_DAYS + 1))
    .to_rfc3339();
    set_all_created_at(&pool, task_id, &old_iso).await;

    let removed = db::prune_request_diagnostics(&pool).await.expect("prune");

    assert_eq!(removed, 5);
    assert_eq!(count_for_task(&pool, task_id).await, 0);
}

#[tokio::test]
async fn prune_caps_independently_per_task() {
    let pool = test_pool("prune-multi").await;
    let task_a = "task-a";
    let task_b = "task-b";
    seed_task(&pool, task_a).await;
    seed_task(&pool, task_b).await;
    // Task A exceeds the cap; task B stays well below it.
    let count_a = db::REQUEST_DIAGNOSTICS_MAX_PER_TASK + 5;
    let count_b = 5;
    for _ in 0..count_a {
        db::insert_request_diagnostic(&pool, &sample_record(task_a))
            .await
            .expect("insert a");
    }
    for _ in 0..count_b {
        db::insert_request_diagnostic(&pool, &sample_record(task_b))
            .await
            .expect("insert b");
    }

    db::prune_request_diagnostics(&pool).await.expect("prune");

    assert_eq!(
        count_for_task(&pool, task_a).await,
        db::REQUEST_DIAGNOSTICS_MAX_PER_TASK
    );
    // Task B should be untouched.
    assert_eq!(count_for_task(&pool, task_b).await, count_b);
    assert_eq!(
        count_total(&pool).await,
        db::REQUEST_DIAGNOSTICS_MAX_PER_TASK + count_b
    );
}

#[tokio::test]
async fn prune_keeps_most_recent_rows_for_task() {
    let pool = test_pool("prune-recency").await;
    let task_id = "task-recency";
    seed_task(&pool, task_id).await;
    // Insert MAX rows normally, then push one extra row whose URL we mark
    // so we can confirm the prune kept the newest (highest id) rows.
    for i in 0..db::REQUEST_DIAGNOSTICS_MAX_PER_TASK {
        let mut record = sample_record(task_id);
        record.url = format!("http://example.com/old/{i}");
        db::insert_request_diagnostic(&pool, &record)
            .await
            .expect("insert old");
    }
    let mut newest = sample_record(task_id);
    newest.url = "http://example.com/newest".to_string();
    db::insert_request_diagnostic(&pool, &newest)
        .await
        .expect("insert newest");

    db::prune_request_diagnostics(&pool).await.expect("prune");

    assert_eq!(
        count_for_task(&pool, task_id).await,
        db::REQUEST_DIAGNOSTICS_MAX_PER_TASK
    );
    // The newest row (highest id) must survive; one of the older rows is gone.
    let kept_newest: Option<String> = sqlx::query_scalar(
        "SELECT url FROM task_requests WHERE task_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(task_id)
    .fetch_one(&pool)
    .await
    .expect("fetch newest url");
    assert_eq!(kept_newest.as_deref(), Some("http://example.com/newest"));
}
