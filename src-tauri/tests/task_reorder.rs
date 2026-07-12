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
    let path = std::env::temp_dir().join(format!("vibe-reorder-{label}-{id}.sqlite"));
    db::connect(&path).await.expect("connect").pool
}

fn sample_task(id: &str, queue_position: i64) -> TaskRecord {
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
        status: TaskStatus::Queued,
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
    }
}

async fn queue_position_of(pool: &sqlx::SqlitePool, id: &str) -> i64 {
    sqlx::query_scalar("SELECT queue_position FROM tasks WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("fetch queue_position")
}

#[tokio::test]
async fn reorder_queued_tasks_reorders_correctly() {
    let pool = test_pool("reorder-correct").await;
    // Three queued tasks with initial positions 1000, 2000, 3000.
    let t1 = sample_task("task-1", 1000);
    let t2 = sample_task("task-2", 2000);
    let t3 = sample_task("task-3", 3000);
    db::insert_task_record(&pool, &t1).await.expect("insert t1");
    db::insert_task_record(&pool, &t2).await.expect("insert t2");
    db::insert_task_record(&pool, &t3).await.expect("insert t3");

    // Reorder to [t3, t1, t2]. Step is 1000, so expected positions are 0/1000/2000.
    db::reorder_queued_tasks(
        &pool,
        &[
            "task-3".to_string(),
            "task-1".to_string(),
            "task-2".to_string(),
        ],
    )
    .await
    .expect("reorder");

    assert_eq!(queue_position_of(&pool, "task-3").await, 0);
    assert_eq!(queue_position_of(&pool, "task-1").await, 1000);
    assert_eq!(queue_position_of(&pool, "task-2").await, 2000);

    pool.close().await;
}

#[tokio::test]
async fn reorder_queued_tasks_empty_input_no_op() {
    let pool = test_pool("reorder-empty").await;
    let t1 = sample_task("task-1", 1000);
    let t2 = sample_task("task-2", 2000);
    db::insert_task_record(&pool, &t1).await.expect("insert t1");
    db::insert_task_record(&pool, &t2).await.expect("insert t2");

    db::reorder_queued_tasks(&pool, &[])
        .await
        .expect("reorder empty");

    // Positions unchanged.
    assert_eq!(queue_position_of(&pool, "task-1").await, 1000);
    assert_eq!(queue_position_of(&pool, "task-2").await, 2000);

    pool.close().await;
}

#[tokio::test]
async fn reorder_queued_tasks_partial_subset_only_updates_passed() {
    let pool = test_pool("reorder-partial").await;
    let t1 = sample_task("task-1", 1000);
    let t2 = sample_task("task-2", 2000);
    let t3 = sample_task("task-3", 3000);
    db::insert_task_record(&pool, &t1).await.expect("insert t1");
    db::insert_task_record(&pool, &t2).await.expect("insert t2");
    db::insert_task_record(&pool, &t3).await.expect("insert t3");

    // Reorder only [t2]. t2 becomes position 0; t1 and t3 keep their positions.
    db::reorder_queued_tasks(&pool, &["task-2".to_string()])
        .await
        .expect("reorder single");

    assert_eq!(queue_position_of(&pool, "task-2").await, 0);
    assert_eq!(queue_position_of(&pool, "task-1").await, 1000);
    assert_eq!(queue_position_of(&pool, "task-3").await, 3000);

    pool.close().await;
}
