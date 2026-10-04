//! Integrity Passport (feature proposal §2.4): checksum state, remote
//! validator evidence, event-derived milestones and resume stats, and the
//! read-only staging FS check. The tauri command is a thin wrapper over
//! `build_integrity_passport`, so the tests exercise that directly.

mod common;

use std::path::Path;

use common::TestPaths;
use tauri_app_lib::{
    commands::tasks::build_integrity_passport,
    db,
    models::{
        HashVerificationStatus, TaskFileRecord, TaskKind, TaskPriority, TaskRecord, TaskStatus,
    },
};

/// Seed defaults: a completed single-file task with resume support and no
/// checksums; individual tests override the fields they care about.
struct SeedTask {
    status: TaskStatus,
    save_dir: String,
    temp_path: Option<String>,
    final_path: Option<String>,
    etag: Option<String>,
    last_modified: Option<String>,
    supports_resume: bool,
    expected_sha256: Option<String>,
    actual_sha256: Option<String>,
    hash_status: HashVerificationStatus,
    hash_verified_at: Option<String>,
}

impl SeedTask {
    fn new(save_dir: &str) -> Self {
        Self {
            status: TaskStatus::Completed,
            save_dir: save_dir.to_string(),
            temp_path: None,
            final_path: None,
            etag: None,
            last_modified: None,
            supports_resume: true,
            expected_sha256: None,
            actual_sha256: None,
            hash_status: HashVerificationStatus::NotRequested,
            hash_verified_at: None,
        }
    }
}

async fn seed_task(pool: &sqlx::SqlitePool, id: &str, seed: &SeedTask) {
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
            save_dir: seed.save_dir.clone(),
            temp_path: seed.temp_path.clone(),
            final_path: seed.final_path.clone(),
            total_size: 10,
            downloaded_bytes: 10,
            status: seed.status,
            etag: seed.etag.clone(),
            last_modified: seed.last_modified.clone(),
            content_type: None,
            supports_resume: seed.supports_resume,
            supports_parallel: false,
            supports_multi_file: false,
            source_key: format!("{id}.example.com"),
            connection_count: 1,
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
            expected_hash_sha256: seed.expected_sha256.clone(),
            actual_hash_sha256: seed.actual_sha256.clone(),
            hash_status: seed.hash_status,
            hash_error: None,
            hash_verified_at: seed.hash_verified_at.clone(),
            created_at: now.clone(),
            updated_at: now,
            files_version: 0,
        },
    )
    .await
    .expect("insert task");
}

async fn seed_checksum(
    pool: &sqlx::SqlitePool,
    id: &str,
    task_id: &str,
    status: &str,
    actual_hash: Option<&str>,
) {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO task_checksums (id, task_id, file_id, algorithm, expected_hash, actual_hash, status, source_kind, verified_at, created_at, updated_at)
         VALUES (?, ?, NULL, 'sha256', 'expected', ?, ?, 'manual', ?, ?, ?)",
    )
    .bind(id)
    .bind(task_id)
    .bind(actual_hash)
    .bind(status)
    .bind(matches!(status, "verified" | "failed").then(|| now.clone()))
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .expect("insert checksum");
}

async fn seed_work_unit(pool: &sqlx::SqlitePool, id: &str, task_id: &str, retry_count: i64) {
    sqlx::query(
        "INSERT INTO task_work_units (id, task_id, unit_kind, status, retry_count) VALUES (?, ?, 'range', 'completed', ?)",
    )
    .bind(id)
    .bind(task_id)
    .bind(retry_count)
    .execute(pool)
    .await
    .expect("insert work unit");
}

async fn seed_task_file(
    pool: &sqlx::SqlitePool,
    id: &str,
    task_id: &str,
    final_path: Option<&str>,
    selected: bool,
) {
    db::insert_task_file_record(
        pool,
        &TaskFileRecord {
            id: id.to_string(),
            task_id: task_id.to_string(),
            relative_path: format!("dir/{id}.bin"),
            file_name: format!("{id}.bin"),
            save_dir: "unused".to_string(),
            temp_path: None,
            final_path: final_path.map(str::to_string),
            total_size: 5,
            downloaded_bytes: 5,
            selected,
            status: TaskStatus::Completed,
            content_type: None,
        },
    )
    .await
    .expect("insert task file");
}

#[tokio::test]
async fn completed_passport_reports_verified_checksum_and_staging_complete() {
    let (_guard, pool) = common::test_pool("passport-complete").await;
    // TestPaths owns its directory and removes it on Drop — bind it to a
    // local that outlives every on-disk assertion below.
    let paths = TestPaths::new("passport-complete");
    std::fs::write(&paths.final_path, b"payload").expect("write output");

    seed_task(
        &pool,
        "t1",
        &SeedTask {
            save_dir: paths.temp.parent().unwrap().to_string_lossy().to_string(),
            final_path: Some(paths.final_path.to_string_lossy().to_string()),
            etag: Some("\"abc\"".to_string()),
            last_modified: Some("Mon, 14 Sep 2026 00:00:00 GMT".to_string()),
            ..SeedTask::new("")
        },
    )
    .await;
    db::insert_task_event(&pool, "t1", "created", None)
        .await
        .expect("event");
    db::insert_task_event(&pool, "t1", "started", None)
        .await
        .expect("event");
    db::insert_task_event(&pool, "t1", "paused", None)
        .await
        .expect("event");
    db::insert_task_event(&pool, "t1", "resumed", None)
        .await
        .expect("event");
    db::insert_task_event(&pool, "t1", "completed", None)
        .await
        .expect("event");
    db::insert_task_event(&pool, "t1", "resumed", None)
        .await
        .expect("event");
    seed_checksum(&pool, "c1", "t1", "verified", Some("verifieddigest")).await;
    seed_work_unit(&pool, "w1", "t1", 2).await;
    seed_work_unit(&pool, "w2", "t1", 1).await;
    sqlx::query("UPDATE tasks SET completed_at = ? WHERE id = ?")
        .bind("2026-09-14T00:00:00Z")
        .bind("t1")
        .execute(&pool)
        .await
        .expect("persist completion time");

    let passport = build_integrity_passport(&pool, "t1")
        .await
        .expect("passport");
    assert_eq!(passport.status, TaskStatus::Completed);
    assert_eq!(
        passport.staging_cleanup,
        tauri_app_lib::models::PassportStagingCleanup::Complete
    );
    assert_eq!(
        passport.checksum_state,
        tauri_app_lib::models::PassportChecksumState::Verified
    );
    assert_eq!(passport.checksums.len(), 1);
    assert_eq!(
        passport.checksums[0].actual_hash.as_deref(),
        Some("verifieddigest")
    );
    assert!(passport.checksums[0].verified_at.is_some());
    assert_eq!(passport.resume_count, 2);
    assert_eq!(passport.segment_retries, 3);
    assert!(passport.started_at.is_some());
    assert!(passport.completed_at.is_some());
    assert_eq!(
        passport.remote_validators,
        vec![
            tauri_app_lib::models::RemoteValidatorKind::Etag,
            tauri_app_lib::models::RemoteValidatorKind::LastModified,
            tauri_app_lib::models::RemoteValidatorKind::Range,
        ]
    );
    assert_eq!(passport.total_bytes.as_deref(), Some("10"));
    assert_eq!(
        passport.final_path.as_deref(),
        Some(paths.final_path.to_string_lossy().as_ref())
    );
}

#[tokio::test]
async fn missing_checksum_reads_not_provided() {
    let (_guard, pool) = common::test_pool("passport-nochecksum").await;
    seed_task(&pool, "t1", &SeedTask::new(".")).await;

    let passport = build_integrity_passport(&pool, "t1")
        .await
        .expect("passport");
    assert!(passport.checksums.is_empty());
    assert_eq!(
        passport.checksum_state,
        tauri_app_lib::models::PassportChecksumState::NotProvided
    );
    // No etag/last_modified but resume supported → only the range validator.
    assert_eq!(
        passport.remote_validators,
        vec![tauri_app_lib::models::RemoteValidatorKind::Range]
    );
}

#[tokio::test]
async fn legacy_hash_columns_fall_back() {
    let (_guard, pool) = common::test_pool("passport-legacy").await;
    seed_task(
        &pool,
        "t1",
        &SeedTask {
            expected_sha256: Some("expected".to_string()),
            actual_sha256: Some("actual".to_string()),
            hash_status: HashVerificationStatus::Verified,
            hash_verified_at: Some("2026-09-15T00:00:00Z".to_string()),
            ..SeedTask::new(".")
        },
    )
    .await;

    let passport = build_integrity_passport(&pool, "t1")
        .await
        .expect("passport");
    assert_eq!(passport.checksums.len(), 1);
    assert_eq!(passport.checksums[0].algorithm, "sha256");
    assert_eq!(passport.checksums[0].actual_hash.as_deref(), Some("actual"));
    assert_eq!(
        passport.checksum_state,
        tauri_app_lib::models::PassportChecksumState::Verified
    );
}

#[tokio::test]
async fn staging_residue_reports_incomplete() {
    let (_guard, pool) = common::test_pool("passport-residue").await;
    let paths = TestPaths::new("passport-residue");
    std::fs::write(&paths.final_path, b"payload").expect("write output");
    std::fs::write(&paths.temp, b"leftover").expect("write temp residue");

    seed_task(
        &pool,
        "t1",
        &SeedTask {
            save_dir: paths.temp.parent().unwrap().to_string_lossy().to_string(),
            temp_path: Some(paths.temp.to_string_lossy().to_string()),
            final_path: Some(paths.final_path.to_string_lossy().to_string()),
            ..SeedTask::new("")
        },
    )
    .await;

    let passport = build_integrity_passport(&pool, "t1")
        .await
        .expect("passport");
    assert_eq!(
        passport.staging_cleanup,
        tauri_app_lib::models::PassportStagingCleanup::Incomplete
    );
}

#[tokio::test]
async fn deleted_output_reports_missing() {
    let (_guard, pool) = common::test_pool("passport-missing").await;
    let paths = TestPaths::new("passport-missing");

    seed_task(
        &pool,
        "t1",
        &SeedTask {
            save_dir: paths.temp.parent().unwrap().to_string_lossy().to_string(),
            final_path: Some(paths.final_path.to_string_lossy().to_string()),
            ..SeedTask::new("")
        },
    )
    .await;

    let passport = build_integrity_passport(&pool, "t1")
        .await
        .expect("passport");
    assert_eq!(
        passport.staging_cleanup,
        tauri_app_lib::models::PassportStagingCleanup::MissingOutput
    );
}

#[tokio::test]
async fn active_task_cleanup_not_applicable() {
    let (_guard, pool) = common::test_pool("passport-active").await;
    let paths = TestPaths::new("passport-active");
    std::fs::write(&paths.temp, b"partial").expect("write temp");

    seed_task(
        &pool,
        "t1",
        &SeedTask {
            status: TaskStatus::Downloading,
            save_dir: paths.temp.parent().unwrap().to_string_lossy().to_string(),
            temp_path: Some(paths.temp.to_string_lossy().to_string()),
            ..SeedTask::new("")
        },
    )
    .await;

    let passport = build_integrity_passport(&pool, "t1")
        .await
        .expect("passport");
    assert_eq!(
        passport.staging_cleanup,
        tauri_app_lib::models::PassportStagingCleanup::NotApplicable
    );
}

#[tokio::test]
async fn pruned_events_leave_milestones_unknown() {
    let (_guard, pool) = common::test_pool("passport-pruned").await;
    seed_task(&pool, "t1", &SeedTask::new(".")).await;

    let passport = build_integrity_passport(&pool, "t1")
        .await
        .expect("passport");
    assert!(passport.started_at.is_none());
    // The task row is seeded as a completed legacy record, but its durable
    // completion field is intentionally absent; the passport must preserve
    // that unknown instead of reconstructing it from events or updated_at.
    assert!(passport.completed_at.is_none());
    assert_eq!(passport.resume_count, 0);
    assert_eq!(passport.segment_retries, 0);
}

#[tokio::test]
async fn persisted_completion_survives_event_retention() {
    let (_guard, pool) = common::test_pool("passport-completion-persisted").await;
    seed_task(&pool, "t1", &SeedTask::new(".")).await;
    sqlx::query("UPDATE tasks SET completed_at = ? WHERE id = ?")
        .bind("2026-09-16T00:00:00Z")
        .bind("t1")
        .execute(&pool)
        .await
        .expect("persist completion time");

    let passport = build_integrity_passport(&pool, "t1")
        .await
        .expect("passport");
    assert_eq!(
        passport.completed_at.as_deref(),
        Some("2026-09-16T00:00:00Z")
    );
    assert!(passport.started_at.is_none());
}

#[tokio::test]
async fn unknown_task_errors() {
    let (_guard, pool) = common::test_pool("passport-absent").await;
    let error = build_integrity_passport(&pool, "ghost")
        .await
        .expect_err("unknown task must error");
    assert!(error.contains("ghost") || error.to_lowercase().contains("not found"));
}

#[tokio::test]
async fn multifile_outputs_use_task_files() {
    let (_guard, pool) = common::test_pool("passport-multifile").await;
    let paths = TestPaths::new("passport-multifile");
    let dir = paths.temp.parent().unwrap().to_path_buf();
    let existing = dir.join("part-a.bin");
    let missing = dir.join("part-b.bin");
    std::fs::write(&existing, b"a").expect("write part a");
    // part-b is intentionally never written.

    seed_task(
        &pool,
        "t1",
        &SeedTask {
            save_dir: dir.to_string_lossy().to_string(),
            ..SeedTask::new("")
        },
    )
    .await;
    seed_task_file(
        &pool,
        "f1",
        "t1",
        Some(existing.to_string_lossy().as_ref()),
        true,
    )
    .await;
    seed_task_file(
        &pool,
        "f2",
        "t1",
        Some(missing.to_string_lossy().as_ref()),
        true,
    )
    .await;
    seed_task_file(&pool, "f3", "t1", None, false).await;

    let passport = build_integrity_passport(&pool, "t1")
        .await
        .expect("passport");
    // One selected file missing → the passport must not claim completion.
    assert_eq!(
        passport.staging_cleanup,
        tauri_app_lib::models::PassportStagingCleanup::MissingOutput
    );

    // Writing the missing file flips the state to complete (no residue).
    std::fs::write(&missing, b"b").expect("write part b");
    let passport = build_integrity_passport(&pool, "t1")
        .await
        .expect("passport");
    assert_eq!(
        passport.staging_cleanup,
        tauri_app_lib::models::PassportStagingCleanup::Complete
    );
    assert!(Path::new(&missing).exists());
}
