//! ARC-02: concurrent final-path reservation.

use std::{
    collections::HashSet,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use sqlx::SqlitePool;
use tauri_app_lib::{
    commands::task_file_planning::{task_temp_file_path, unique_final_path_among},
    db,
    models::{
        task::now_iso, HashVerificationStatus, TaskKind, TaskPriority, TaskRecord, TaskStatus,
    },
};
use tokio::sync::Barrier;

async fn test_pool(label: &str) -> SqlitePool {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-path-reserve-{label}-{id}.sqlite"));
    db::connect(&path)
        .await
        .expect("database connect with migrations")
        .pool
}

fn queued_task(id: &str, final_path: &str, temp_path: &str) -> TaskRecord {
    let now = now_iso();
    TaskRecord {
        id: id.to_string(),
        url: format!("https://example.com/{id}.bin"),
        final_url: Some(format!("https://example.com/{id}.bin")),
        protocol: "http".to_string(),
        task_kind: TaskKind::SingleFile,
        file_name: "file.bin".to_string(),
        save_dir: std::env::temp_dir().to_string_lossy().to_string(),
        temp_path: Some(temp_path.to_string()),
        final_path: Some(final_path.to_string()),
        total_size: 4,
        downloaded_bytes: 0,
        status: TaskStatus::Queued,
        etag: None,
        last_modified: None,
        content_type: None,
        supports_resume: true,
        supports_parallel: true,
        supports_multi_file: false,
        source_key: "example.com".to_string(),
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
    }
}

#[tokio::test]
async fn concurrent_same_name_creates_reserve_unique_final_paths() {
    // N concurrent reservations must yield unique final paths (CI-stable N=20).
    const N: usize = 20;
    let pool = Arc::new(test_pool("concurrent").await);
    let dir = std::env::temp_dir().join(format!(
        "vibe-concurrent-paths-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create dir");
    let barrier = Arc::new(Barrier::new(N));
    let mut handles = Vec::with_capacity(N);

    for index in 0..N {
        let pool = Arc::clone(&pool);
        let dir = dir.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            let mut last_error = None;
            for attempt in 0..64 {
                let task_id = format!("task-{index}-{attempt}");
                let mut tx = pool.begin().await.expect("begin");
                let reserved = db::list_reserved_final_paths(&mut *tx)
                    .await
                    .expect("list reserved");
                let final_path = unique_final_path_among(&dir, "file.bin", &reserved);
                let temp_path = task_temp_file_path(&final_path, &task_id);
                let task = queued_task(
                    &task_id,
                    &final_path.to_string_lossy(),
                    &temp_path.to_string_lossy(),
                );
                match db::insert_task_record_in_tx(&mut tx, &task).await {
                    Ok(()) => match tx.commit().await {
                        Ok(()) => return final_path,
                        Err(error) => {
                            last_error = Some(error.to_string());
                            continue;
                        }
                    },
                    Err(error) => {
                        last_error = Some(error);
                        continue;
                    }
                }
            }
            panic!(
                "failed to reserve path for worker {index}: {:?}",
                last_error
            );
        }));
    }

    let mut paths = HashSet::new();
    for handle in handles {
        let path = handle.await.expect("join");
        assert!(
            paths.insert(path.clone()),
            "duplicate final path reserved: {}",
            path.display()
        );
        assert!(
            path.to_string_lossy().contains("file"),
            "unexpected path {}",
            path.display()
        );
    }
    assert_eq!(paths.len(), N);

    let count: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT final_path) FROM tasks")
        .fetch_one(pool.as_ref())
        .await
        .expect("count");
    assert_eq!(count, N as i64);

    let _ = std::fs::remove_dir_all(&dir);
    pool.close().await;
}

#[tokio::test]
async fn final_path_active_unique_index_exists() {
    let pool = test_pool("index").await;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name='idx_tasks_final_path_active')",
    )
    .fetch_one(&pool)
    .await
    .expect("check index");
    assert!(exists);
    pool.close().await;
}
