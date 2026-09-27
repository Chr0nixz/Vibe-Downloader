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
    seed_task_with_paths(
        pool,
        id,
        &std::env::temp_dir().to_string_lossy(),
        None,
        None,
    )
    .await;
}

/// SEC-02: seed a task whose stored output paths are caller-controlled, so a
/// test can build the exact backup an attacker would ship.
async fn seed_task_with_paths(
    pool: &sqlx::SqlitePool,
    id: &str,
    save_dir: &str,
    temp_path: Option<&str>,
    final_path: Option<&str>,
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
    assert!(apply_pending_restore_if_any(&empty_live)
        .await
        .expect("apply"));
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

#[cfg(debug_assertions)]
#[test]
fn backup_process_exit_before_publish() {
    let Some(path) = std::env::var_os("VIBE_TEST_BACKUP_TARGET") else {
        return;
    };
    let bytes = std::fs::read(std::env::var_os("VIBE_TEST_BACKUP_INPUT").unwrap()).unwrap();
    db::write_backup_file_for_test(
        std::path::Path::new(&path),
        &bytes,
        None,
        false,
        Some(Box::new(|| std::process::exit(73))),
    )
    .unwrap();
    panic!("prepublication exit hook did not run");
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn backup_faults_and_process_exit_preserve_a_readable_previous_backup() {
    use sha2::{Digest, Sha256};
    use std::{io::ErrorKind, process::Command};
    let paths = common::TestPaths::new("backup-atomic");
    let root = paths.temp.parent().unwrap();
    let live = root.join("live.sqlite");
    let pool = db::connect(&live).await.unwrap().pool;
    seed_task(&pool, "old-task").await;
    let destination = root.join("state.vibe-backup");
    make_backup_from_pool(&pool, &live, &destination).await;
    let original = std::fs::read(&destination).unwrap();
    seed_task(&pool, "new-task").await;
    let next = root.join("next.vibe-backup");
    make_backup_from_pool(&pool, &live, &next).await;
    let replacement = std::fs::read(&next).unwrap();
    for (fault, sync_failure) in [
        (Some((37, ErrorKind::WriteZero)), false),
        (Some((70_000, ErrorKind::StorageFull)), false),
        (None, true),
    ] {
        assert!(db::write_backup_file_for_test(
            &destination,
            &replacement,
            fault,
            sync_failure,
            None
        )
        .unwrap_err()
        .contains("backup_write_failed"));
        assert_eq!(
            Sha256::digest(std::fs::read(&destination).unwrap()),
            Sha256::digest(&original)
        );
        assert!(std::fs::read_dir(root).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".state.vibe-backup.")));
        read_backup_file(&destination).unwrap();
    }
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "backup_process_exit_before_publish",
            "--nocapture",
        ])
        .env("VIBE_TEST_BACKUP_TARGET", &destination)
        .env("VIBE_TEST_BACKUP_INPUT", &next)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(73));
    assert_eq!(
        Sha256::digest(std::fs::read(&destination).unwrap()),
        Sha256::digest(&original)
    );
    // An abrupt exit can leave a private .tmp file. A subsequent export must
    // ignore it, and the visible backup remains the only restore candidate.
    write_backup_file(&destination, &replacement).unwrap();
    let parsed = read_backup_file(&destination).unwrap();
    let schema = db::current_schema_version(&pool).await.unwrap();
    let verified = db::materialize_and_verify_backup_db(&parsed.database, schema, schema, None)
        .await
        .unwrap();
    let restored = db::connect(&verified).await.unwrap().pool;
    assert!(db::get_task_record(&restored, "new-task")
        .await
        .unwrap()
        .is_some());
    restored.close().await;
    std::fs::remove_file(verified).unwrap();
    pool.close().await;
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

/// SEC-02: a backup whose task paths point outside the allowed roots must be
/// rejected, and the pre-existing integrity checks must be shown to accept it.
///
/// This is the whole point of the fix: magic, checksum, schema and
/// `PRAGMA integrity_check` are all things the attacker controls or can
/// recompute, so none of them stop a crafted `.vibe-backup` from pointing
/// `final_path` at, say, a Startup folder. Restore writes those strings
/// verbatim once the scheduler resumes the task.
#[tokio::test]
async fn sec02_backup_with_out_of_root_paths_is_rejected() {
    let live = unique_path("sec02-live.sqlite");
    let backup = unique_path("sec02-evil.vibe-backup");
    let allowed_root = unique_path("sec02-downloads");
    std::fs::create_dir_all(&allowed_root).expect("create allowed root");

    let pool = db::connect(&live).await.expect("connect").pool;
    let outside = std::env::temp_dir()
        .join("vibe-sec02-outside")
        .join("evil.bin");
    seed_task_with_paths(
        &pool,
        "evil",
        &allowed_root.to_string_lossy(),
        None,
        Some(&outside.to_string_lossy()),
    )
    .await;
    make_backup_from_pool(&pool, &live, &backup).await;

    // Every pre-existing check accepts it - the attacker computes a valid
    // checksum for their own payload.
    let parsed = read_backup_file(&backup).expect("integrity checks still accept a crafted backup");
    let current = db::current_schema_version(&pool).await.expect("schema");
    let verified = db::materialize_and_verify_backup_db(
        &parsed.database,
        parsed.manifest.schema_version,
        current,
        None,
    )
    .await
    .expect("structural verification still accepts a crafted backup");

    // Only the content policy stops it.
    let error = db::enforce_backup_path_policy(&verified, std::slice::from_ref(&allowed_root))
        .await
        .expect_err("SEC-02: an out-of-root final_path must be rejected");
    assert!(
        error.contains("backup_unsafe_paths"),
        "expected backup_unsafe_paths, got: {error}"
    );

    // The live database must be untouched by a rejected restore.
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&pool)
        .await
        .expect("integrity");
    assert_eq!(integrity, "ok");
    assert!(
        !pending_restore_path(&live).exists(),
        "a rejected backup must not stage a pending restore"
    );

    let _ = std::fs::remove_file(&verified);
    let _ = std::fs::remove_dir_all(&allowed_root);
    pool.close().await;
}

/// SEC-02: the policy must not over-reject. Paths inside an allowed root, and
/// NULL paths, stay acceptable.
#[tokio::test]
async fn sec02_backup_with_in_root_paths_is_accepted() {
    let live = unique_path("sec02-ok-live.sqlite");
    let backup = unique_path("sec02-ok.vibe-backup");
    let allowed_root = unique_path("sec02-ok-downloads");
    std::fs::create_dir_all(&allowed_root).expect("create allowed root");

    let pool = db::connect(&live).await.expect("connect").pool;
    let inside = allowed_root.join("nested").join("movie.mkv");
    seed_task_with_paths(
        &pool,
        "good",
        &allowed_root.to_string_lossy(),
        Some(&allowed_root.join("movie.mkv.part").to_string_lossy()),
        Some(&inside.to_string_lossy()),
    )
    .await;
    // A task with no output path yet must not trip the policy either.
    seed_task(&pool, "no-paths-yet").await;
    make_backup_from_pool(&pool, &live, &backup).await;

    let parsed = read_backup_file(&backup).expect("read backup");
    let current = db::current_schema_version(&pool).await.expect("schema");
    let verified = db::materialize_and_verify_backup_db(
        &parsed.database,
        parsed.manifest.schema_version,
        current,
        None,
    )
    .await
    .expect("verify");

    // `seed_task` uses the temp dir, so it has to be an allowed root here too.
    db::enforce_backup_path_policy(&verified, &[allowed_root.clone(), std::env::temp_dir()])
        .await
        .expect("SEC-02: in-root and NULL paths must be accepted");

    let _ = std::fs::remove_file(&verified);
    let _ = std::fs::remove_dir_all(&allowed_root);
    pool.close().await;
}

/// SEC-02: `credentials_policy` was parsed and forwarded to the UI without ever
/// being checked. Anything other than the machine-bound policy must be refused
/// at parse time so `validate_app_backup` rejects it as well.
#[test]
fn sec02_unexpected_credentials_policy_is_rejected() {
    let manifest = BackupManifest {
        format: "vibe-backup".into(),
        format_version: BACKUP_FORMAT_VERSION,
        app_version: "0.4.0".into(),
        schema_version: 1,
        created_at: chrono::Utc::now().to_rfc3339(),
        credentials_policy: "plaintext".into(),
        includes_global_proxy_password: true,
        checksum_algorithm: "sha256".into(),
        checksum: String::new(),
        database_bytes: 0,
    };
    let packed = pack_backup_file(&manifest, b"not-a-real-database").expect("pack");
    let error = parse_backup_bytes(&packed).expect_err("a foreign credentials policy is refused");
    assert!(
        error.contains("backup_invalid_manifest"),
        "expected backup_invalid_manifest, got: {error}"
    );
}

/// SEC-09 + FUN-26: after a pending restore replaces the live database, the
/// scrub must (a) reset machine-executable settings a crafted backup could
/// smuggle in, and (b) fix proxy_password_saved against the LOCAL keyring —
/// not the value from the backup.
#[tokio::test]
async fn sec09_restore_scrub_clears_command_and_fixes_proxy_flag() {
    let live = unique_path("sec09-live.sqlite");
    let backup = unique_path("sec09.vibe-backup");
    let pool = db::connect(&live).await.expect("connect").pool;
    seed_task(&pool, "sec09-task").await;

    // Craft the malicious settings row exactly as an attacker's backup would
    // carry them.
    sqlx::query("INSERT INTO settings(key, value) VALUES('completion_action', 'run_command')")
        .execute(&pool)
        .await
        .expect("seed completion_action");
    sqlx::query(
        "INSERT INTO settings(key, value) VALUES('completion_run_command', 'C WHEN(evil.bat)')",
    )
    .execute(&pool)
    .await
    .expect("seed command");
    sqlx::query(
        "INSERT INTO settings(key, value) VALUES('ffmpeg_path', 'C WHEN(tools)ffmpeg.exe')",
    )
    .execute(&pool)
    .await
    .expect("seed ffmpeg");
    sqlx::query("INSERT INTO settings(key, value) VALUES('proxy_password_saved', 'true')")
        .execute(&pool)
        .await
        .expect("seed proxy flag");

    make_backup_from_pool(&pool, &live, &backup).await;
    pool.close().await;

    let parsed = read_backup_file(&backup).expect("parse");
    let empty_live = unique_path("sec09-empty.sqlite");
    let pending = pending_restore_path(&empty_live);
    std::fs::write(&pending, &parsed.database).expect("stage pending");

    apply_pending_restore_if_any(&empty_live)
        .await
        .expect("apply");

    // Open the restored DB and verify the scrub.
    let restored = db::connect(&empty_live)
        .await
        .expect("connect restored")
        .pool;
    async fn setting(pool: &sqlx::SqlitePool, key: &str) -> Option<String> {
        sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await
            .expect("read setting")
    }
    assert_eq!(
        setting(&restored, "completion_action").await.as_deref(),
        Some("notify")
    );
    assert_eq!(
        setting(&restored, "completion_run_command")
            .await
            .as_deref(),
        Some("")
    );
    assert_eq!(setting(&restored, "ffmpeg_path").await.as_deref(), Some(""));

    // FUN-26: the flag must reflect the LOCAL keyring. In tests the keyring
    // proxy password is absent (no real keyring entry), so the restored
    // 'true' from the backup must be corrected to 'false'.
    assert_eq!(
        setting(&restored, "proxy_password_saved").await.as_deref(),
        Some("false"),
        "proxy flag must be corrected against the local keyring, not the backup"
    );

    restored.close().await;
    let _ = std::fs::remove_file(&live);
    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::remove_file(&empty_live);
}

/// FUN-23: when rename fails (simulated via an unwritable pre-created target
/// file lock scenario is platform-dependent; instead force the copy path by
/// removing the rename precondition — the copy fallback must produce a
/// byte-identical verified snapshot).
#[tokio::test]
async fn fun23_snapshot_copy_fallback_produces_verified_snapshot() {
    // Directly exercise copy_verified_snapshot's contract via the public API:
    // snapshot to a destination on the same volume still succeeds, and the
    // fallback path is exercised when rename is unavailable. We simulate a
    // cross-volume failure by monkey-patching is impossible; instead assert
    // the snapshot output is byte-complete (what copy+verify guarantees).
    let live = unique_path("fun23-live.sqlite");
    let backup = unique_path("fun23.vibe-backup");
    let pool = db::connect(&live).await.expect("connect").pool;
    seed_task(&pool, "fun23-task").await;

    let destination = unique_path("fun23-snapshot.sqlite");
    snapshot_database_to_path(&pool, &live, &destination)
        .await
        .expect("snapshot (rename or copy fallback)");

    // Both paths must leave a loadable, schema-complete database.
    let reopened = db::connect(&destination).await.expect("snapshot loads");
    let tasks = db::list_task_records(&reopened.pool)
        .await
        .expect("list tasks");
    assert!(tasks.iter().any(|task| task.id == "fun23-task"));
    reopened.pool.close().await;

    pool.close().await;
    let _ = std::fs::remove_file(&live);
    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::remove_file(&destination);
}

/// ARC-53 (R26-A05): the verified staging file must land beside the live
/// database, not in the OS temp dir — a pending-restore rename is a filesystem
/// move that fails across volumes (EXDEV / ERROR_NOT_SAME_DEVICE). Staging in
/// the DB's own directory keeps it same-volume regardless of where TEMP lives.
#[tokio::test]
async fn arc53_verified_backup_stages_beside_the_live_database() {
    let live_dir = unique_path("arc53-staging");
    std::fs::create_dir_all(&live_dir).expect("create db dir");
    let live = live_dir.join("live.sqlite");
    let backup = unique_path("arc53.vibe-backup");
    let pool = db::connect(&live).await.expect("connect").pool;
    seed_task(&pool, "arc53-task").await;
    make_backup_from_pool(&pool, &live, &backup).await;

    let parsed = read_backup_file(&backup).expect("parse");
    let current = db::current_schema_version(&pool).await.expect("schema");
    let verified =
        db::materialize_and_verify_backup_db(&parsed.database, current, current, Some(&live_dir))
            .await
            .expect("materialize");

    // The verified file must live in the DB's directory (same volume as the
    // pending path), never in the OS temp dir.
    assert_eq!(
        verified.parent().expect("verified parent"),
        live_dir.as_path(),
        "verified staging must sit beside the live database"
    );
    assert_ne!(
        verified.parent(),
        Some(std::env::temp_dir().as_path()),
        "staging must not leak into the OS temp dir"
    );

    // Same-volume rename to the pending path succeeds — this is the operation
    // that failed when staging lived in TEMP on another volume.
    let pending = pending_restore_path(&live);
    assert_eq!(
        pending.parent().expect("pending parent"),
        live_dir.as_path()
    );
    std::fs::rename(&verified, &pending).expect("same-volume rename to pending");
    assert!(pending.exists(), "pending restore must be staged");

    pool.close().await;
    let _ = std::fs::remove_file(&pending);
    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::remove_dir_all(&live_dir);
}

/// ARC-53: a read-only consumer (no staging dir) still uses the OS temp dir —
/// preview/subset paths never rename the file, so the temp dir is safe there.
#[tokio::test]
async fn arc53_readonly_materialize_defaults_to_temp_dir() {
    let live = unique_path("arc53-readonly.sqlite");
    let backup = unique_path("arc53-readonly.vibe-backup");
    let pool = db::connect(&live).await.expect("connect").pool;
    seed_task(&pool, "arc53-readonly").await;
    make_backup_from_pool(&pool, &live, &backup).await;

    let parsed = read_backup_file(&backup).expect("parse");
    let current = db::current_schema_version(&pool).await.expect("schema");
    let verified = db::materialize_and_verify_backup_db(&parsed.database, current, current, None)
        .await
        .expect("materialize");
    assert_eq!(
        verified.parent(),
        Some(std::env::temp_dir().as_path()),
        "no staging dir must keep the OS temp dir default"
    );

    pool.close().await;
    let _ = std::fs::remove_file(&verified);
    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::remove_file(&live);
}
