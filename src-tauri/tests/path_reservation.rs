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
        task::now_iso, HashVerificationStatus, TaskFileRecord, TaskKind, TaskPriority, TaskRecord,
        TaskStatus,
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

/// Build a file row whose `final_path` is intentionally independent of the task
/// id, so several tasks planning the same output collide on
/// `idx_task_files_final_path_selected`.
fn shared_task_file(task_id: &str, dir: &std::path::Path, index: usize) -> TaskFileRecord {
    let name = format!("shared-{index}.bin");
    TaskFileRecord {
        id: format!("{task_id}-f{index}"),
        task_id: task_id.to_string(),
        relative_path: name.clone(),
        file_name: name.clone(),
        save_dir: dir.to_string_lossy().to_string(),
        temp_path: Some(
            dir.join(format!("{name}.{task_id}.vibe-downloading"))
                .to_string_lossy()
                .to_string(),
        ),
        final_path: Some(dir.join(&name).to_string_lossy().to_string()),
        total_size: 1,
        downloaded_bytes: 0,
        selected: true,
        status: TaskStatus::Queued,
        content_type: None,
    }
}

/// ARC-20: a UNIQUE conflict on any file row must roll the task row back too.
///
/// Before the fix the task row was committed first and the file rows were then
/// inserted one at a time on the pool. A conflict part-way through the loop
/// therefore left a committed task with a truncated file list, which the
/// scheduler would download and report as successful - silent data loss.
///
/// Every worker here plans identical file paths but distinct task paths, so the
/// only possible conflict is on `task_files`. Exactly one worker may win, and
/// the losers must leave nothing at all behind.
#[tokio::test]
async fn arc20_file_row_conflict_rolls_back_the_task_row() {
    const WORKERS: usize = 32;
    const FILES_PER_TASK: usize = 8;

    let pool = test_pool("arc20-atomic").await;
    let dir = std::env::temp_dir().join(format!("vibe-arc20-atomic-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create dir");

    let barrier = Arc::new(Barrier::new(WORKERS));
    let mut handles = Vec::new();
    for index in 0..WORKERS {
        let pool = pool.clone();
        let barrier = barrier.clone();
        let dir = dir.clone();
        handles.push(tokio::spawn(async move {
            let task_id = format!("task-{index}");
            let task = queued_task(
                &task_id,
                &dir.join(format!("{task_id}.bin")).to_string_lossy(),
                &dir.join(format!("{task_id}.part")).to_string_lossy(),
            );
            let files: Vec<TaskFileRecord> = (0..FILES_PER_TASK)
                .map(|file_index| shared_task_file(&task_id, &dir, file_index))
                .collect();
            barrier.wait().await;
            db::insert_task_with_files(&pool, &task, &files).await
        }));
    }

    let mut succeeded = 0usize;
    for handle in handles {
        if handle.await.expect("join worker").is_ok() {
            succeeded += 1;
        }
    }
    assert_eq!(
        succeeded, 1,
        "exactly one worker can win the file-path UNIQUE race"
    );

    let tasks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks")
        .fetch_one(&pool)
        .await
        .expect("count tasks");
    let files: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_files")
        .fetch_one(&pool)
        .await
        .expect("count files");
    assert_eq!(
        tasks, 1,
        "losing workers must not leave a committed task row"
    );
    assert_eq!(
        files, FILES_PER_TASK as i64,
        "the winner's file list must be complete and losers must leave no rows"
    );

    // No task may exist with an incomplete file list.
    let partial: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks t WHERE (SELECT COUNT(*) FROM task_files f WHERE f.task_id = t.id) <> ?",
    )
    .bind(FILES_PER_TASK as i64)
    .fetch_one(&pool)
    .await
    .expect("count partial tasks");
    assert_eq!(partial, 0, "no task may be left with a partial file list");

    let _ = std::fs::remove_dir_all(&dir);
    pool.close().await;
}

/// ARC-21: concurrent multi-file creates with non-colliding paths must all
/// commit. `insert_task_with_files` uses `BEGIN IMMEDIATE`; a DEFERRED
/// transaction that reads before writing can fail with SQLITE_BUSY_SNAPSHOT,
/// which `busy_timeout` does not retry and the create path did not recognize.
#[tokio::test]
async fn arc21_concurrent_multi_file_creates_all_commit() {
    const WORKERS: usize = 32;
    const FILES_PER_TASK: usize = 8;

    let pool = test_pool("arc21-immediate").await;
    let dir = std::env::temp_dir().join(format!("vibe-arc21-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create dir");

    let barrier = Arc::new(Barrier::new(WORKERS));
    let mut handles = Vec::new();
    for index in 0..WORKERS {
        let pool = pool.clone();
        let barrier = barrier.clone();
        let dir = dir.clone();
        handles.push(tokio::spawn(async move {
            let task_id = format!("task-{index}");
            let task = queued_task(
                &task_id,
                &dir.join(format!("{task_id}.bin")).to_string_lossy(),
                &dir.join(format!("{task_id}.part")).to_string_lossy(),
            );
            // Per-task subdirectory keeps every final_path distinct.
            let task_dir = dir.join(&task_id);
            let files: Vec<TaskFileRecord> = (0..FILES_PER_TASK)
                .map(|file_index| shared_task_file(&task_id, &task_dir, file_index))
                .collect();
            barrier.wait().await;
            db::insert_task_with_files(&pool, &task, &files).await
        }));
    }

    for (index, handle) in handles.into_iter().enumerate() {
        handle
            .await
            .expect("join worker")
            .unwrap_or_else(|error| panic!("worker {index} failed: {error}"));
    }

    let tasks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks")
        .fetch_one(&pool)
        .await
        .expect("count tasks");
    let files: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_files")
        .fetch_one(&pool)
        .await
        .expect("count files");
    assert_eq!(tasks, WORKERS as i64);
    assert_eq!(files, (WORKERS * FILES_PER_TASK) as i64);

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
