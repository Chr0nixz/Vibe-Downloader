//! Backup & Migration Center (feature proposal §3.7): content inventory,
//! SEC-02 path scan reporting, migration path remap, safe-subset restore, and
//! the post-restore report sidecar. The command layer is thin over these
//! `db::` entry points, so the tests exercise them directly.

mod common;

use std::path::{Path, PathBuf};

use tauri_app_lib::{
    db::{
        self, apply_pending_restore_if_any, count_contents, dismiss_restore_report,
        materialize_and_verify_backup_db, pack_backup_file, pending_restore_path, read_backup_file,
        read_restore_report, remap_backup_paths, restore_subset, verify_backup_integrity,
        write_backup_file, write_staging_meta, RestoreStagingMeta, BACKUP_FORMAT_VERSION,
        CREDENTIALS_POLICY_MACHINE_BOUND,
    },
    models::{
        backup::{BackupSubsetSelection, RestoreReport},
        HashVerificationStatus, TaskKind, TaskPriority, TaskRecord, TaskStatus,
    },
};

async fn seed_task_full(
    pool: &sqlx::SqlitePool,
    id: &str,
    status: TaskStatus,
    save_dir: &str,
    temp_path: Option<&str>,
    final_path: Option<&str>,
    error_message: Option<&str>,
) {
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
            save_dir: save_dir.to_string(),
            temp_path: temp_path.map(str::to_string),
            final_path: final_path.map(str::to_string),
            total_size: 10,
            downloaded_bytes: 10,
            status,
            etag: None,
            last_modified: None,
            content_type: None,
            supports_resume: true,
            supports_parallel: false,
            supports_multi_file: false,
            source_key: format!("{id}.example.com"),
            connection_count: 1,
            speed_bps: 123,
            task_speed_limit_bps: None,
            priority: TaskPriority::Normal,
            queue_position: 0,
            category_key: None,
            obey_schedule: true,
            health_summary: Some("seeded".to_string()),
            error_message: error_message.map(str::to_string),
            error_code: error_message.map(|_| "resume_unavailable".to_string()),
            recovery_actions: Vec::new(),
            retry_after_at: error_message.map(|_| "2026-01-01T00:00:00Z".to_string()),
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

async fn seed_checksum(pool: &sqlx::SqlitePool, id: &str, task_id: &str) {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO task_checksums (id, task_id, file_id, algorithm, expected_hash, status, source_kind, created_at, updated_at)
         VALUES (?, ?, NULL, 'sha256', 'deadbeef', 'pending', 'manual', ?, ?)",
    )
    .bind(id)
    .bind(task_id)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .expect("insert checksum");
}

async fn seed_event(pool: &sqlx::SqlitePool, task_id: &str, event_type: &str) {
    sqlx::query("INSERT INTO task_events (task_id, event_type, created_at) VALUES (?, ?, ?)")
        .bind(task_id)
        .bind(event_type)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(pool)
        .await
        .expect("insert event");
}

async fn set_setting(pool: &sqlx::SqlitePool, key: &str, value: &str) {
    sqlx::query("INSERT OR REPLACE INTO settings(key, value) VALUES (?, ?)")
        .bind(key)
        .bind(value)
        .execute(pool)
        .await
        .expect("set setting");
}

async fn get_setting(pool: &sqlx::SqlitePool, key: &str) -> Option<String> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .expect("read setting");
    row.map(|(value,)| value)
}

async fn seed_rule(pool: &sqlx::SqlitePool, id: &str, name: &str) {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO classification_rules (id, name, enabled, position, match_kind, pattern, target_subdir, created_at, updated_at)
         VALUES (?, ?, 1, 0, 'extension', 'bin', ?, ?, ?)",
    )
    .bind(id)
    .bind(name)
    .bind(format!("subdir-{name}"))
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .expect("insert rule");
}

fn unique_path(label: &str) -> PathBuf {
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!("vibe-backup-center-{label}-{id}"))
}

async fn make_backup_file(pool: &sqlx::SqlitePool, db_file: &Path, dest: &Path) {
    let snapshot = dest.with_extension("sqlite.tmp");
    db::snapshot_database_to_path(pool, db_file, &snapshot)
        .await
        .expect("snapshot");
    let database = std::fs::read(&snapshot).expect("read snapshot");
    let _ = std::fs::remove_file(&snapshot);
    let schema_version = db::current_schema_version(pool).await.expect("schema");
    let manifest = tauri_app_lib::db::BackupManifest {
        format: "vibe-backup".into(),
        format_version: BACKUP_FORMAT_VERSION,
        app_version: "0.5.0".into(),
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

async fn materialize_backup(backup: &Path) -> PathBuf {
    let parsed = read_backup_file(backup).expect("parse backup");
    let current = parsed.manifest.schema_version;
    materialize_and_verify_backup_db(&parsed.database, current, current)
        .await
        .expect("materialize")
}

#[tokio::test]
async fn contents_inventory_matches_between_live_and_backup() {
    let live = unique_path("contents-live.sqlite");
    let backup = unique_path("contents.vibe-backup");
    let pool = db::connect(&live).await.expect("connect").pool;

    seed_task_full(
        &pool,
        "inv-done",
        TaskStatus::Completed,
        &std::env::temp_dir().to_string_lossy(),
        None,
        None,
        None,
    )
    .await;
    seed_task_full(
        &pool,
        "inv-fail",
        TaskStatus::Failed,
        &std::env::temp_dir().to_string_lossy(),
        None,
        None,
        Some("boom"),
    )
    .await;
    seed_checksum(&pool, "chk-inv", "inv-done").await;
    seed_event(&pool, "inv-fail", "failed").await;
    seed_rule(&pool, "rule-inv", "Inventory").await;
    set_setting(
        &pool,
        "browser_capture_settings",
        r#"{"siteRules":[{"id":"s1"},{"id":"s2"},{"id":"s3"}]}"#,
    )
    .await;

    let live_contents = count_contents(&pool).await.expect("live contents");
    assert_eq!(live_contents.tasks_total, 2);
    assert_eq!(live_contents.tasks_completed, 1);
    assert_eq!(live_contents.tasks_failed, 1);
    assert_eq!(live_contents.tasks_with_checksums, 1);
    assert_eq!(live_contents.tasks_with_request_headers, 0);
    assert_eq!(live_contents.classification_rules, 1);
    assert_eq!(live_contents.site_rules, 3);
    assert_eq!(live_contents.task_events, 1);

    make_backup_file(&pool, &live, &backup).await;
    let parsed = read_backup_file(&backup).expect("parse");
    let current = parsed.manifest.schema_version;
    let verified = materialize_and_verify_backup_db(&parsed.database, current, current)
        .await
        .expect("materialize");

    let url = format!("sqlite://{}?mode=ro", verified.display());
    let snapshot = sqlx::SqlitePool::connect(&url)
        .await
        .expect("open snapshot");
    let backup_contents = count_contents(&snapshot).await.expect("backup contents");
    let preview = db::backup_settings_preview(&snapshot)
        .await
        .expect("settings preview");
    snapshot.close().await;

    assert_eq!(live_contents.tasks_total, backup_contents.tasks_total);
    assert_eq!(live_contents.site_rules, backup_contents.site_rules);
    assert_eq!(live_contents.task_events, backup_contents.task_events);
    // The seeded default_save_dir (whatever connect() wrote) is visible.
    assert!(
        preview.default_save_dir.is_empty() || Path::new(&preview.default_save_dir).is_absolute()
    );
    let _ = std::fs::remove_file(&verified);
    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::remove_file(&live);
}

#[tokio::test]
async fn scan_reports_offenders_and_remap_relocates_paths() {
    let live = unique_path("remap-live.sqlite");
    let backup = unique_path("remap.vibe-backup");
    let pool = db::connect(&live).await.expect("connect").pool;

    let old_default = unique_path("remap-old-dl");
    let elsewhere = unique_path("remap-elsewhere");
    let a_dir = old_default.join("movies");
    let _ = std::fs::create_dir_all(&a_dir);
    seed_task_full(
        &pool,
        "remap-a",
        TaskStatus::Completed,
        &a_dir.to_string_lossy(),
        Some(&a_dir.join("a.bin.tmp").to_string_lossy()),
        Some(&a_dir.join("a.bin").to_string_lossy()),
        None,
    )
    .await;
    seed_task_full(
        &pool,
        "remap-b",
        TaskStatus::Completed,
        &elsewhere.to_string_lossy(),
        None,
        Some(&elsewhere.join("b.bin").to_string_lossy()),
        None,
    )
    .await;
    set_setting(&pool, "default_save_dir", &old_default.to_string_lossy()).await;

    make_backup_file(&pool, &live, &backup).await;
    let verified = materialize_backup(&backup).await;

    let new_root = unique_path("remap-new-root");
    // Without remap the SEC-02 policy must stay fail-closed.
    let rejected = db::enforce_backup_path_policy(&verified, std::slice::from_ref(&new_root)).await;
    assert!(rejected.is_err(), "out-of-root snapshot must be rejected");
    let scan_url = format!("sqlite://{}?mode=ro", verified.display());
    let scan_pool = sqlx::SqlitePool::connect(&scan_url).await.expect("open");
    let scan = db::scan_backup_path_policy(&scan_pool, std::slice::from_ref(&new_root))
        .await
        .expect("scan");
    scan_pool.close().await;
    assert!(
        scan.offenders.len() >= 5,
        "expected all stored paths to offend"
    );
    assert_eq!(scan.offending_save_dirs.len(), 2);

    let remapped = remap_backup_paths(&verified, &new_root, Some(&old_default))
        .await
        .expect("remap");
    assert_eq!(
        remapped, 5,
        "three values for task A, two for task B (no temp)"
    );

    // After the remap the policy passes against the new root alone.
    db::enforce_backup_path_policy(&verified, std::slice::from_ref(&new_root))
        .await
        .expect("policy passes after remap");
    verify_backup_integrity(&verified)
        .await
        .expect("integrity after remap");

    let url = format!("sqlite://{}?mode=rw", verified.display());
    let check = sqlx::SqlitePool::connect(&url).await.expect("open");
    let save_dir: String = sqlx::query_scalar("SELECT save_dir FROM tasks WHERE id = 'remap-a'")
        .fetch_one(&check)
        .await
        .expect("task a");
    assert_eq!(
        Path::new(&save_dir),
        new_root.join("movies").as_path(),
        "default-tree structure is preserved"
    );
    let final_b: String = sqlx::query_scalar("SELECT final_path FROM tasks WHERE id = 'remap-b'")
        .fetch_one(&check)
        .await
        .expect("task b");
    assert!(Path::new(&final_b).starts_with(new_root.join("migrated")));
    let default_dir: String =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'default_save_dir'")
            .fetch_one(&check)
            .await
            .expect("default dir");
    assert_eq!(Path::new(&default_dir), new_root.as_path());
    check.close().await;

    let _ = std::fs::remove_file(&verified);
    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::remove_file(&live);
    let _ = std::fs::remove_dir_all(&old_default);
    let _ = std::fs::remove_dir_all(&new_root);
}

#[tokio::test]
async fn subset_tasks_only_skips_existing_and_normalizes_active() {
    let live = unique_path("subset-live.sqlite");
    let backup = unique_path("subset.vibe-backup");

    // The backup: one completed task that also exists on the live side, one
    // mid-flight task with satellite rows.
    let backup_db = unique_path("subset-pool.sqlite");
    let backup_pool = db::connect(&backup_db).await.expect("connect").pool;
    seed_task_full(
        &backup_pool,
        "dup-task",
        TaskStatus::Completed,
        &std::env::temp_dir().to_string_lossy(),
        None,
        None,
        None,
    )
    .await;
    seed_task_full(
        &backup_pool,
        "flying-task",
        TaskStatus::Downloading,
        &std::env::temp_dir().to_string_lossy(),
        None,
        None,
        Some("stalled"),
    )
    .await;
    seed_checksum(&backup_pool, "chk-dup", "dup-task").await;
    seed_checksum(&backup_pool, "chk-flying", "flying-task").await;
    seed_event(&backup_pool, "flying-task", "progress").await;
    let _ = count_contents(&backup_pool).await.expect("contents");
    make_backup_file(&backup_pool, &backup_db, &backup).await;
    backup_pool.close().await;
    let _ = std::fs::remove_file(&backup_db);

    // The live side already knows "dup-task"; nothing else overlaps.
    let pool = db::connect(&live).await.expect("connect").pool;
    seed_task_full(
        &pool,
        "keep-live",
        TaskStatus::Completed,
        &std::env::temp_dir().to_string_lossy(),
        None,
        None,
        None,
    )
    .await;
    seed_task_full(
        &pool,
        "dup-task",
        TaskStatus::Completed,
        &std::env::temp_dir().to_string_lossy(),
        None,
        None,
        None,
    )
    .await;

    let verified = materialize_backup(&backup).await;
    let result = restore_subset(
        &live,
        &verified,
        BackupSubsetSelection {
            tasks: true,
            rules: false,
            settings: false,
        },
        &[std::env::temp_dir()],
    )
    .await
    .expect("subset restore");
    let _ = std::fs::remove_file(&verified);

    assert_eq!(result.tasks_inserted, 1, "only flying-task is new");
    assert_eq!(result.tasks_skipped, 1, "dup-task collided");
    assert_eq!(result.tasks_normalized, 1, "flying-task was mid-flight");
    assert_eq!(result.rules_inserted, 0);
    assert_eq!(result.settings_replaced, 0);

    let flying = db::get_task_record(&pool, "flying-task")
        .await
        .expect("read")
        .expect("task restored");
    assert_eq!(flying.status, TaskStatus::Paused);
    assert!(flying.error_message.is_none());
    assert!(flying.error_code.is_none());
    assert!(flying.retry_after_at.is_none());

    // Satellite rows: only the new task's rows came over.
    let checksums: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_checksums WHERE task_id IN ('dup-task', 'flying-task')",
    )
    .fetch_one(&pool)
    .await
    .expect("count checksums");
    assert_eq!(checksums, 1, "flying-task checksum only");
    let events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM task_events WHERE task_id = 'flying-task'")
            .fetch_one(&pool)
            .await
            .expect("count events");
    assert_eq!(events, 1);

    // Idempotent re-run: everything already exists now.
    let verified2 = materialize_backup(&backup).await;
    let again = restore_subset(
        &live,
        &verified2,
        BackupSubsetSelection {
            tasks: true,
            rules: false,
            settings: false,
        },
        &[std::env::temp_dir()],
    )
    .await
    .expect("second subset restore");
    let _ = std::fs::remove_file(&verified2);
    assert_eq!(again.tasks_inserted, 0);
    assert_eq!(again.tasks_skipped, 2);
    assert_eq!(again.tasks_normalized, 0);

    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::remove_file(&live);
}

#[tokio::test]
async fn subset_rules_and_settings_replace_then_scrub() {
    let live = unique_path("subset-rs-live.sqlite");
    let backup = unique_path("subset-rs.vibe-backup");

    let backup_db = unique_path("subset-rs-pool.sqlite");
    let backup_pool = db::connect(&backup_db).await.expect("connect").pool;
    seed_rule(&backup_pool, "rule-new", "Fresh").await;
    seed_rule(&backup_pool, "rule-shared", "FromBackup").await;
    set_setting(&backup_pool, "ffmpeg_path", "/old/tools/ffmpeg").await;
    set_setting(&backup_pool, "completion_action", "shutdown").await;
    set_setting(&backup_pool, "default_save_dir", "/old-dl").await;
    // FUN-26: the backup claims a proxy password this machine does not have.
    set_setting(&backup_pool, "proxy_password_saved", "true").await;
    set_setting(
        &backup_pool,
        "browser_capture_settings",
        r#"{"siteRules":[{"id":"s9"}]}"#,
    )
    .await;
    make_backup_file(&backup_pool, &backup_db, &backup).await;
    backup_pool.close().await;
    let _ = std::fs::remove_file(&backup_db);

    let pool = db::connect(&live).await.expect("connect").pool;
    seed_rule(&pool, "rule-shared", "LiveOriginal").await;

    let verified = materialize_backup(&backup).await;
    let result = restore_subset(
        &live,
        &verified,
        BackupSubsetSelection {
            tasks: false,
            rules: true,
            settings: true,
        },
        &[],
    )
    .await
    .expect("subset restore");
    let _ = std::fs::remove_file(&verified);

    assert_eq!(result.rules_inserted, 1);
    assert_eq!(result.rules_skipped, 1);
    assert!(result.settings_replaced > 0);

    // The shared rule kept its live identity (additive restore).
    let shared_name: String =
        sqlx::query_scalar("SELECT name FROM classification_rules WHERE id = 'rule-shared'")
            .fetch_one(&pool)
            .await
            .expect("shared rule");
    assert_eq!(shared_name, "LiveOriginal");
    let rules: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM classification_rules")
        .fetch_one(&pool)
        .await
        .expect("count rules");
    assert_eq!(rules, 2);

    // SEC-09 scrub ran on the live database: machine-bound values reset.
    assert_eq!(
        get_setting(&pool, "ffmpeg_path").await.as_deref(),
        Some(""),
        "ffmpeg path must be scrubbed"
    );
    assert_eq!(
        get_setting(&pool, "completion_action").await.as_deref(),
        Some("notify")
    );
    // Unscrubbed settings travel verbatim.
    assert_eq!(
        get_setting(&pool, "default_save_dir").await.as_deref(),
        Some("/old-dl")
    );
    // Capture rules came with the rules subset.
    let capture = get_setting(&pool, "browser_capture_settings")
        .await
        .expect("capture row");
    assert!(capture.contains("s9"));

    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::remove_file(&live);
}

#[tokio::test]
async fn subset_empty_selection_is_rejected() {
    let live = unique_path("subset-empty-live.sqlite");
    let backup = unique_path("subset-empty.vibe-backup");
    let backup_db = unique_path("subset-empty-pool.sqlite");
    let pool = db::connect(&backup_db).await.expect("connect").pool;
    make_backup_file(&pool, &backup_db, &backup).await;
    pool.close().await;
    let _ = std::fs::remove_file(&backup_db);

    let _ = db::connect(&live).await.expect("connect");
    let verified = materialize_backup(&backup).await;
    let error = restore_subset(
        &live,
        &verified,
        BackupSubsetSelection {
            tasks: false,
            rules: false,
            settings: false,
        },
        &[],
    )
    .await
    .expect_err("empty selection must be rejected");
    assert!(error.contains("backup_subset_empty"), "got: {error}");
    let _ = std::fs::remove_file(&verified);
    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::remove_file(&live);
}

#[tokio::test]
async fn restore_report_written_and_dismissable() {
    let backup = unique_path("report.vibe-backup");
    let backup_db = unique_path("report-pool.sqlite");
    let backup_pool = db::connect(&backup_db).await.expect("connect").pool;
    let missing_dir = unique_path("report-missing-save/nested");
    assert!(!missing_dir.exists());
    seed_task_full(
        &backup_pool,
        "report-task",
        TaskStatus::Failed,
        &missing_dir.to_string_lossy(),
        None,
        None,
        Some("boom"),
    )
    .await;
    set_setting(&backup_pool, "ffmpeg_path", "/old/tools/ffmpeg").await;
    set_setting(&backup_pool, "completion_action", "shutdown").await;
    set_setting(&backup_pool, "proxy_password_saved", "true").await;
    make_backup_file(&backup_pool, &backup_db, &backup).await;
    backup_pool.close().await;
    let _ = std::fs::remove_file(&backup_db);

    let parsed = read_backup_file(&backup).expect("parse");
    let empty_live = unique_path("report-applied.sqlite");
    let pending = pending_restore_path(&empty_live);
    std::fs::write(&pending, &parsed.database).expect("stage pending");
    write_staging_meta(
        &pending,
        &RestoreStagingMeta {
            pre_restore_backup_path: "/rollback/vibe.db.bak-1".into(),
            backup_created_at: parsed.manifest.created_at.clone(),
            schema_version: parsed.manifest.schema_version,
        },
    )
    .expect("write meta");

    assert!(apply_pending_restore_if_any(&empty_live)
        .await
        .expect("apply"));

    let report: RestoreReport = read_restore_report(&empty_live).expect("report present");
    assert!(report.ffmpeg_was_configured);
    assert!(report.completion_action_reset);
    // No global proxy password in the local keyring, but the backup claimed one.
    assert!(report.global_proxy_needs_reentry);
    assert_eq!(
        report.pre_restore_backup_path.as_deref(),
        Some("/rollback/vibe.db.bak-1")
    );
    assert_eq!(
        report.backup_created_at.as_deref(),
        Some(parsed.manifest.created_at.as_str())
    );
    assert!(report
        .missing_save_dirs
        .iter()
        .any(|dir| dir == &missing_dir.to_string_lossy().to_string()));
    assert!(report.missing_save_dirs_total >= 1);
    assert!(dismiss_restore_report(&empty_live));
    assert!(read_restore_report(&empty_live).is_none());

    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::remove_file(&empty_live);
}

#[tokio::test]
async fn subset_tasks_respects_sec02_path_policy() {
    let live = unique_path("subset-sec02-live.sqlite");
    let backup = unique_path("subset-sec02.vibe-backup");
    let backup_db = unique_path("subset-sec02-pool.sqlite");
    let backup_pool = db::connect(&backup_db).await.expect("connect").pool;
    let outside = unique_path("outside-roots");
    seed_task_full(
        &backup_pool,
        "smuggled-task",
        TaskStatus::Completed,
        &outside.to_string_lossy(),
        None,
        Some(&outside.join("smuggled.bin").to_string_lossy()),
        None,
    )
    .await;
    make_backup_file(&backup_pool, &backup_db, &backup).await;
    backup_pool.close().await;
    let _ = std::fs::remove_file(&backup_db);

    let _ = db::connect(&live).await.expect("connect");
    let verified = materialize_backup(&backup).await;
    // A root that does NOT contain the snapshot's paths must reject the whole
    // subset before any row reaches the live database.
    let unrelated_root = unique_path("unrelated-root");
    let error = restore_subset(
        &live,
        &verified,
        BackupSubsetSelection {
            tasks: true,
            rules: false,
            settings: false,
        },
        &[unrelated_root],
    )
    .await
    .expect_err("out-of-root snapshot must be rejected");
    assert!(error.contains("backup_unsafe_paths"), "got: {error}");
    // Nothing landed.
    let pool = db::connect(&live).await.expect("reconnect").pool;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(count, 0, "no task rows may land on a policy rejection");
    pool.close().await;
    let _ = std::fs::remove_file(&verified);
    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::remove_file(&live);
}

#[tokio::test]
async fn subset_restore_is_refused_while_a_pending_restore_waits() {
    let live = unique_path("subset-pending-live.sqlite");
    let backup = unique_path("subset-pending.vibe-backup");
    let backup_db = unique_path("subset-pending-pool.sqlite");
    let backup_pool = db::connect(&backup_db).await.expect("connect").pool;
    seed_task_full(
        &backup_pool,
        "pending-task",
        TaskStatus::Completed,
        &std::env::temp_dir().to_string_lossy(),
        None,
        None,
        None,
    )
    .await;
    make_backup_file(&backup_pool, &backup_db, &backup).await;
    backup_pool.close().await;
    let _ = std::fs::remove_file(&backup_db);

    let pool = db::connect(&live).await.expect("connect").pool;
    // Stage a whole-file restore exactly as the restore command would.
    let parsed = read_backup_file(&backup).expect("parse");
    let pending = pending_restore_path(&live);
    std::fs::write(&pending, &parsed.database).expect("stage pending");

    let verified = materialize_backup(&backup).await;
    let error = restore_subset(
        &live,
        &verified,
        BackupSubsetSelection {
            tasks: true,
            rules: false,
            settings: false,
        },
        &[std::env::temp_dir()],
    )
    .await
    .expect_err("pending swap would silently discard the merge");
    assert!(error.contains("backup_restore_pending"), "got: {error}");
    // The live database is untouched and the pending file survives.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(count, 0);
    assert!(pending.exists());
    let _ = std::fs::remove_file(&verified);
    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::remove_file(&live);
}

#[tokio::test]
async fn remap_with_unchanged_root_is_a_reported_noop() {
    let live = unique_path("remap-noop-live.sqlite");
    let backup = unique_path("remap-noop.vibe-backup");
    let pool = db::connect(&live).await.expect("connect").pool;

    let old_default = unique_path("remap-noop-dl");
    let _ = std::fs::create_dir_all(&old_default);
    seed_task_full(
        &pool,
        "remap-noop-a",
        TaskStatus::Completed,
        &old_default.to_string_lossy(),
        None,
        Some(&old_default.join("a.bin").to_string_lossy()),
        None,
    )
    .await;
    set_setting(&pool, "default_save_dir", &old_default.to_string_lossy()).await;

    make_backup_file(&pool, &live, &backup).await;
    let verified = materialize_backup(&backup).await;

    // Remapping onto the OLD default root must not rewrite anything.
    let remapped = remap_backup_paths(&verified, &old_default, Some(&old_default))
        .await
        .expect("remap");
    assert_eq!(
        remapped, 0,
        "identical values must not be counted or rewritten"
    );
    verify_backup_integrity(&verified)
        .await
        .expect("integrity after noop remap");
    db::enforce_backup_path_policy(&verified, &[old_default.clone(), std::env::temp_dir()])
        .await
        .expect("policy still passes");

    let _ = std::fs::remove_file(&verified);
    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::remove_file(&live);
    let _ = std::fs::remove_dir_all(&old_default);
}
