//! FUN-16: versioned `.vibe-backup` create / validate / restore contracts.

mod common;

use std::path::PathBuf;

use tauri_app_lib::{
    db::{
        self, apply_pending_restore_if_any, pack_backup_file, parse_backup_bytes,
        pending_restore_path, read_backup_file, snapshot_database_to_path, write_backup_file,
        BackupManifest, BACKUP_FORMAT_VERSION, CREDENTIALS_POLICY_MACHINE_BOUND,
    },
    models::{HashVerificationStatus, TaskKind, TaskPriority, TaskRecord, TaskStatus},
};

async fn seed_task(pool: &sqlx::SqlitePool, id: &str) {
    let now = chrono::Utc::now().to_rfc3339();
    db::insert_task_record(
        pool,
        &TaskRecord {
            id: id.to_string(),
            url: format!("https://example.com/{id}.bin"),
            final_url: Some(format!("https://example.com/{id}.bin")),
            protocol: "http".to_string(),
            task_kind: TaskKind::SingleFile,
            file_name: format!("{id}.bin"),
            save_dir: std::env::temp_dir().to_string_lossy().to_string(),
            temp_path: None,
            final_path: None,
            total_size: 10,
            downloaded_bytes: 10,
            status: TaskStatus::Completed,
            etag: None,
            last_modified: None,
            content_type: None,
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
            obey_schedule: true,
            health_summary: Some("Completed".to_string()),
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
        },
    )
    .await
    .expect("insert task");
}

fn unique_path(label: &str) -> PathBuf {
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!("vibe-fun16-{label}-{id}"))
}

async fn make_backup_from_pool(
    pool: &sqlx::SqlitePool,
    live_db: &std::path::Path,
    dest: &std::path::Path,
) {
    let snapshot = dest.with_extension("sqlite.tmp");
    snapshot_database_to_path(pool, live_db, &snapshot)
        .await
        .expect("snapshot");
    let database = std::fs::read(&snapshot).expect("read snapshot");
    let _ = std::fs::remove_file(&snapshot);
    let schema_version = db::current_schema_version(pool).await.expect("schema");
    let manifest = BackupManifest {
        format: "vibe-backup".into(),
        format_version: BACKUP_FORMAT_VERSION,
        app_version: "0.3.0".into(),
        schema_version,
        created_at: chrono::Utc::now().to_rfc3339(),
        credentials_policy: CREDENTIALS_POLICY_MACHINE_BOUND.into(),
        includes_global_proxy_password: false,
        checksum_algorithm: "sha256".into(),
        checksum: String::new(),
        database_bytes: 0,
    };
    let packed = pack_backup_file(&manifest, &database).expect("pack");
    write_backup_file(dest, &packed).expect("write");
}

#[tokio::test]
async fn fun16_backup_round_trip_preserves_tasks() {
    let live = unique_path("live.sqlite");
    let backup = unique_path("roundtrip.vibe-backup");
    let pool = db::connect(&live).await.expect("connect").pool;
    seed_task(&pool, "fun16-task-a").await;
    make_backup_from_pool(&pool, &live, &backup).await;
    pool.close().await;

    let parsed = read_backup_file(&backup).expect("parse");
    assert_eq!(
        parsed.manifest.credentials_policy,
        CREDENTIALS_POLICY_MACHINE_BOUND
    );

    let restore_target = unique_path("restore.sqlite");
    std::fs::write(&restore_target, &parsed.database).expect("write restore db");
    // Stage as pending next to an empty live path and apply.
    let empty_live = unique_path("empty-live.sqlite");
    let pending = pending_restore_path(&empty_live);
    std::fs::rename(&restore_target, &pending).expect("stage pending");
    assert!(apply_pending_restore_if_any(&empty_live).expect("apply"));
    let restored = db::connect(&empty_live).await.expect("reconnect").pool;
    let task = db::get_task_record(&restored, "fun16-task-a")
        .await
        .expect("read")
        .expect("task exists after restore");
    assert_eq!(task.status, TaskStatus::Completed);
    restored.close().await;
    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::remove_file(&live);
    let _ = std::fs::remove_file(&empty_live);
}

#[tokio::test]
async fn fun16_corrupt_checksum_is_rejected_without_touching_live() {
    let live = unique_path("live-corrupt.sqlite");
    let backup = unique_path("corrupt.vibe-backup");
    let pool = db::connect(&live).await.expect("connect").pool;
    seed_task(&pool, "keep-me").await;
    make_backup_from_pool(&pool, &live, &backup).await;

    let mut bytes = std::fs::read(&backup).expect("read");
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&backup, &bytes).expect("overwrite corrupt");

    let err = parse_backup_bytes(&bytes).expect_err("corrupt must fail");
    assert!(
        err.contains("backup_checksum_mismatch") || err.contains("checksum"),
        "got {err}"
    );
    assert!(!pending_restore_path(&live).exists());
    let task = db::get_task_record(&pool, "keep-me")
        .await
        .expect("read")
        .expect("live untouched");
    assert_eq!(task.id, "keep-me");
    pool.close().await;
}

#[tokio::test]
async fn fun16_failed_restore_staging_leaves_live_integrity_ok() {
    let live = unique_path("live-fail.sqlite");
    let pool = db::connect(&live).await.expect("connect").pool;
    seed_task(&pool, "still-here").await;

    // A truncated backup must fail before staging pending restore.
    let bad = unique_path("truncated.vibe-backup");
    std::fs::write(&bad, b"VIBE").expect("write truncated");
    let err = read_backup_file(&bad).expect_err("truncated");
    assert!(err.contains("backup_corrupt") || err.contains("truncated") || err.contains("Backup"));

    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&pool)
        .await
        .expect("integrity");
    assert_eq!(integrity, "ok");
    let task = db::get_task_record(&pool, "still-here")
        .await
        .expect("read")
        .expect("exists");
    assert_eq!(task.id, "still-here");
    pool.close().await;
}
