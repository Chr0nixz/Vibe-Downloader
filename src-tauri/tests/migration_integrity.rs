use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::{AssertSqlSafe, Row};
use tauri_app_lib::{
    db,
    models::{
        task::now_iso, HashVerificationStatus, TaskFileRecord, TaskKind, TaskPriority, TaskRecord,
        TaskStatus,
    },
};

async fn test_pool(label: &str) -> sqlx::SqlitePool {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-migration-{label}-{id}.sqlite"));
    db::connect(&path)
        .await
        .expect("database connect with migrations")
        .pool
}

#[tokio::test]
async fn full_migration_on_fresh_database() {
    let pool = test_pool("fresh").await;

    // Baseline consolidation: the original 14 incremental migrations were
    // merged into a single `001_init.sql`, followed by metalink health, HLS track
    // selection, ARC-01 source_key unique drop, and ARC-02 final_path unique.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .expect("count migrations");
    assert_eq!(count, 5, "expected exactly 5 migrations, got {count}");

    pool.close().await;
}

#[tokio::test]
async fn key_tables_exist_after_migration() {
    let pool = test_pool("schema").await;

    let tables = [
        "tasks",
        "task_work_units",
        "task_files",
        "settings",
        "task_credentials",
        "task_proxy_settings",
        "task_checksums",
        "hls_tasks",
        "dash_tasks",
        "metalink_tasks",
        "metalink_resources",
        "sftp_known_hosts",
        "torrent_runtime_snapshots",
        "task_events",
        "task_requests",
        "classification_rules",
    ];
    for table in &tables {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .unwrap_or(false);
        assert!(exists, "table '{table}' should exist after migration");
    }

    pool.close().await;
}

#[tokio::test]
async fn tasks_table_has_expected_columns() {
    let pool = test_pool("columns").await;

    let columns = sqlx::query("PRAGMA table_info(tasks)")
        .fetch_all(&pool)
        .await
        .expect("pragma table_info(tasks)");
    let column_names: Vec<String> = columns
        .iter()
        .map(|row| row.get::<String, _>("name"))
        .collect();

    for expected in &[
        "id",
        "url",
        "save_dir",
        "status",
        "priority",
        "queue_position",
        "source_key",
        "total_size",
        "downloaded_bytes",
        "speed_bps",
    ] {
        assert!(
            column_names.iter().any(|c| c == expected),
            "tasks.{expected} should exist, columns: {column_names:?}"
        );
    }

    pool.close().await;
}

#[tokio::test]
async fn baseline_contains_all_evolved_columns() {
    // Guards against accidental column loss during the baseline consolidation.
    // Each entry below was originally added by an ALTER TABLE in migrations
    // 002-014 and must remain present in the merged baseline.
    let pool = test_pool("evolved").await;

    let cases: &[(&str, &[&str])] = &[
        (
            "tasks",
            &[
                "task_speed_limit_bps",
                "priority",
                "queue_position",
                "category_key",
                "obey_schedule",
                "files_version",
            ],
        ),
        ("task_requests", &["if_range_header"]),
        (
            "hls_segments",
            &[
                "init_map_uri",
                "init_map_local_path",
                "init_map_byte_range_start",
                "init_map_byte_range_length",
            ],
        ),
        (
            "torrent_tasks",
            &[
                "seeding_enabled",
                "seed_ratio_limit",
                "seed_time_limit_seconds",
            ],
        ),
        (
            "torrent_runtime_snapshots",
            &[
                "piece_count",
                "piece_bitfield_base64",
                "dht_status",
                "trackers_json",
                "seeding_enabled",
                "seeding_state",
                "last_error_code",
                "last_error_message",
            ],
        ),
    ];

    for (table, expected_cols) in cases {
        // Table names are test-hardened literals (not user input), so the
        // dynamic PRAGMA string is safe from injection. AssertSqlSafe bypasses
        // sqlx 0.9's SqlSafeStr audit gate.
        let rows = sqlx::query(AssertSqlSafe(format!("PRAGMA table_info({table})")))
            .fetch_all(&pool)
            .await
            .unwrap_or_else(|e| panic!("pragma table_info({table}): {e}"));
        let actual: Vec<String> = rows
            .iter()
            .map(|row| row.get::<String, _>("name"))
            .collect();
        for col in *expected_cols {
            assert!(
                actual.iter().any(|c| c == col),
                "{table}.{col} missing from baseline, actual: {actual:?}"
            );
        }
    }

    pool.close().await;
}

#[tokio::test]
async fn source_key_active_unique_index_is_absent() {
    // ARC-01: host-level source_key must not uniquely constrain active tasks.
    let pool = test_pool("dedup").await;

    let indexes =
        sqlx::query("SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='tasks'")
            .fetch_all(&pool)
            .await
            .expect("list task indexes");
    let index_names: Vec<String> = indexes
        .iter()
        .map(|row| row.get::<String, _>("name"))
        .collect();

    assert!(
        !index_names
            .iter()
            .any(|n| n == "idx_tasks_source_key_active"),
        "legacy unique index 'idx_tasks_source_key_active' must not exist, got: {index_names:?}"
    );

    pool.close().await;
}

fn sample_task(id: &str, url: &str, source_key: &str, status: TaskStatus) -> TaskRecord {
    let now = now_iso();
    TaskRecord {
        id: id.to_string(),
        url: url.to_string(),
        final_url: Some(url.to_string()),
        protocol: "http".to_string(),
        task_kind: TaskKind::SingleFile,
        file_name: format!("{id}.bin"),
        save_dir: std::env::temp_dir().to_string_lossy().to_string(),
        temp_path: None,
        final_path: None,
        total_size: 0,
        downloaded_bytes: 0,
        status,
        etag: None,
        last_modified: None,
        content_type: None,
        supports_resume: true,
        supports_parallel: true,
        supports_multi_file: false,
        source_key: source_key.to_string(),
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
async fn same_host_different_urls_can_coexist_when_active() {
    // ARC-01: two different URLs sharing a host-level source_key may be active together.
    let pool = test_pool("same-host").await;
    let host_key = "cdn.example.com";

    db::insert_task_record(
        &pool,
        &sample_task(
            "task-a",
            "https://cdn.example.com/a.bin",
            host_key,
            TaskStatus::Queued,
        ),
    )
    .await
    .expect("insert first same-host task");
    db::insert_task_record(
        &pool,
        &sample_task(
            "task-b",
            "https://cdn.example.com/b.bin",
            host_key,
            TaskStatus::Downloading,
        ),
    )
    .await
    .expect("insert second same-host task");
    db::insert_task_record(
        &pool,
        &sample_task(
            "task-c",
            "https://cdn.example.com/c.bin",
            host_key,
            TaskStatus::Paused,
        ),
    )
    .await
    .expect("insert third same-host task");

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE source_key = ? AND status IN ('queued', 'downloading', 'paused')",
    )
    .bind(host_key)
    .fetch_one(&pool)
    .await
    .expect("count same-host active tasks");
    assert_eq!(count, 3);

    pool.close().await;
}

#[tokio::test]
async fn migration_004_drops_legacy_source_key_unique_on_upgrade() {
    // Simulate a pre-004 database that still has the unique index, then
    // reconnect so sqlx reapplies migration 004.
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-migration-arc01-upgrade-{id}.sqlite"));

    let pool = db::connect(&path).await.expect("first connect").pool;
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version = 4")
        .execute(&pool)
        .await
        .expect("rewind migration 004");
    sqlx::query(
        r#"
        CREATE UNIQUE INDEX idx_tasks_source_key_active
            ON tasks (source_key)
            WHERE source_key IS NOT NULL
              AND source_key != ''
              AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
        "#,
    )
    .execute(&pool)
    .await
    .expect("recreate legacy unique index");
    pool.close().await;

    let pool = db::connect(&path).await.expect("upgrade connect").pool;
    let index_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name='idx_tasks_source_key_active')",
    )
    .fetch_one(&pool)
    .await
    .expect("check index");
    assert!(
        !index_exists,
        "migration 004 must drop idx_tasks_source_key_active on upgrade"
    );

    db::insert_task_record(
        &pool,
        &sample_task(
            "upgrade-a",
            "https://cdn.example.com/a.bin",
            "cdn.example.com",
            TaskStatus::Queued,
        ),
    )
    .await
    .expect("insert after upgrade");
    db::insert_task_record(
        &pool,
        &sample_task(
            "upgrade-b",
            "https://cdn.example.com/b.bin",
            "cdn.example.com",
            TaskStatus::Queued,
        ),
    )
    .await
    .expect("second same-host insert after upgrade");

    pool.close().await;
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
}

#[tokio::test]
async fn duplicate_bt_info_hash_still_rejected() {
    // ARC-01: BT uniqueness remains on torrent_tasks.info_hash, not tasks.source_key.
    let pool = test_pool("bt-dedup").await;
    let info_hash = "0123456789abcdef0123456789abcdef01234567";

    let mut first = sample_task(
        "bt-task-1",
        "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
        &format!("bt:{info_hash}"),
        TaskStatus::Queued,
    );
    first.protocol = "bt".to_string();
    db::insert_task_record(&pool, &first)
        .await
        .expect("insert first bt task");
    db::upsert_torrent_task(
        &pool,
        &first.id,
        db::TorrentTaskUpsert {
            info_hash,
            name: "first",
            magnet_uri: Some(&first.url),
            torrent_blob: None,
            piece_length: 262144,
            piece_count: 1,
            private: false,
            trackers_json: None,
            seeding_enabled: false,
            seed_ratio_limit: None,
            seed_time_limit_seconds: None,
        },
    )
    .await
    .expect("upsert first torrent row");

    let mut second = sample_task(
        "bt-task-2",
        "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=other",
        &format!("bt:{info_hash}"),
        TaskStatus::Queued,
    );
    second.protocol = "bt".to_string();
    db::insert_task_record(&pool, &second)
        .await
        .expect("insert second bt task row");
    let conflict = db::upsert_torrent_task(
        &pool,
        &second.id,
        db::TorrentTaskUpsert {
            info_hash,
            name: "second",
            magnet_uri: Some(&second.url),
            torrent_blob: None,
            piece_length: 262144,
            piece_count: 1,
            private: false,
            trackers_json: None,
            seeding_enabled: false,
            seed_ratio_limit: None,
            seed_time_limit_seconds: None,
        },
    )
    .await;
    assert!(
        conflict.is_err(),
        "duplicate info_hash must still be rejected by torrent_tasks UNIQUE"
    );

    pool.close().await;
}

#[tokio::test]
async fn migration_002_adds_metalink_health_columns() {
    // Validates the four columns added by migration 002_metalink_health
    // are present on a fresh database and have the expected defaults:
    //  - last_attempt_at (TEXT, NULL on existing rows)
    //  - cooldown_until (TEXT, NULL on existing rows)
    //  - avg_speed_bps (INTEGER NOT NULL DEFAULT 0)
    //  - supports_range (INTEGER NOT NULL DEFAULT 1)
    let pool = test_pool("metalink-health").await;

    let rows = sqlx::query("PRAGMA table_info(metalink_resources)")
        .fetch_all(&pool)
        .await
        .expect("pragma table_info(metalink_resources)");
    let columns: Vec<String> = rows
        .iter()
        .map(|row| row.get::<String, _>("name"))
        .collect();
    for expected in [
        "last_attempt_at",
        "cooldown_until",
        "avg_speed_bps",
        "supports_range",
    ] {
        assert!(
            columns.iter().any(|c| c == expected),
            "metalink_resources.{expected} missing after migration, actual: {columns:?}"
        );
    }

    // Insert a row via the same DB helpers the engine uses, then read the
    // new columns back to verify their NOT NULL + DEFAULT behaviour.
    let now = now_iso();
    let task_id = "metalink-health-task".to_string();
    let task = TaskRecord {
        id: task_id.clone(),
        url: "https://example.com/m.meta4".to_string(),
        final_url: Some("https://example.com/m.meta4".to_string()),
        protocol: "metalink".to_string(),
        task_kind: TaskKind::Manifest,
        file_name: "m.bin".to_string(),
        save_dir: std::env::temp_dir().to_string_lossy().to_string(),
        temp_path: None,
        final_path: None,
        total_size: 0,
        downloaded_bytes: 0,
        status: TaskStatus::Queued,
        etag: None,
        last_modified: None,
        content_type: None,
        supports_resume: true,
        supports_parallel: true,
        supports_multi_file: false,
        source_key: "metalink-health".to_string(),
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
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");

    let file_id = "metalink-health-file".to_string();
    db::insert_task_file_record(
        &pool,
        &TaskFileRecord {
            id: file_id.clone(),
            task_id: task_id.clone(),
            relative_path: "m.bin".to_string(),
            file_name: "m.bin".to_string(),
            save_dir: std::env::temp_dir().to_string_lossy().to_string(),
            temp_path: Some(
                std::env::temp_dir()
                    .join("m.bin.tmp")
                    .to_string_lossy()
                    .to_string(),
            ),
            final_path: Some(
                std::env::temp_dir()
                    .join("m.bin")
                    .to_string_lossy()
                    .to_string(),
            ),
            total_size: 0,
            downloaded_bytes: 0,
            status: TaskStatus::Queued,
            selected: true,
            content_type: None,
        },
    )
    .await
    .expect("insert task_file");

    db::insert_metalink_resource(
        &pool,
        db::MetalinkResourceInsert {
            id: "metalink-health-resource",
            task_id: &task_id,
            file_id: &file_id,
            url: "https://example.com/m.bin",
            priority: 1,
            location: None,
        },
    )
    .await
    .expect("insert metalink_resource");

    let (supports_range, avg_speed, last_attempt, cooldown): (
        i64,
        i64,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT supports_range, avg_speed_bps, last_attempt_at, cooldown_until \
             FROM metalink_resources WHERE id = 'metalink-health-resource'",
    )
    .fetch_one(&pool)
    .await
    .expect("fetch defaults");
    assert_eq!(
        supports_range, 1,
        "supports_range default should be 1 (true)"
    );
    assert_eq!(avg_speed, 0, "avg_speed_bps default should be 0");
    assert!(
        last_attempt.is_none(),
        "last_attempt_at default should be NULL"
    );
    assert!(cooldown.is_none(), "cooldown_until default should be NULL");

    pool.close().await;
}

#[tokio::test]
async fn migration_idempotent_on_reconnect() {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-migration-idempotent-{id}.sqlite"));

    let pool1 = db::connect(&path).await.expect("first connect").pool;
    pool1.close().await;

    let pool2 = db::connect(&path).await.expect("second connect").pool;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&pool2)
        .await
        .expect("count");
    assert_eq!(
        count, 5,
        "expected exactly 5 migrations on reconnect, got {count}"
    );
    pool2.close().await;

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
}

// --- A-1: Dirty / VersionTooOld recovery -------------------------------------

/// A dirty migration must preserve the original database and produce a
/// verified backup before the application offers an explicit reset.
#[tokio::test]
async fn dirty_migration_requires_explicit_recovery() {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-migration-dirty-{id}.sqlite"));

    // First connect: applies both migrations cleanly.
    let pool = db::connect(&path).await.expect("first connect").pool;
    // Simulate a previous interrupted migration by flagging the latest
    // migration row dirty. sqlx::migrate! refuses to proceed when it sees
    // a dirty row, returning `MigrateError::Dirty(version)`.
    sqlx::query("UPDATE _sqlx_migrations SET success = 0")
        .execute(&pool)
        .await
        .expect("flag dirty");
    pool.close().await;

    let recovery = match db::connect_for_startup(&path)
        .await
        .expect("inspect dirty migration")
    {
        db::DatabaseConnectOutcome::RecoveryRequired(recovery) => recovery,
        db::DatabaseConnectOutcome::Ready(_) => panic!("dirty migration must not start normally"),
    };
    assert_eq!(recovery.reason, "migration_dirty");
    assert!(recovery.backup_verified);
    let backup_path = recovery.backup_path.expect("verified backup path");
    assert!(backup_path.is_file());
    assert!(path.is_file(), "the original database must remain on disk");

    let original = sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=ro", path.display()))
        .await
        .expect("open preserved original");
    let dirty_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 0")
            .fetch_one(&original)
            .await
            .expect("count preserved dirty rows");
    assert!(dirty_count > 0);
    original.close().await;
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    let _ = std::fs::remove_file(&backup_path);
}

/// Validates the A-1 downgrade recovery path. When a previous (newer) version
/// of the app recorded a migration that the current (older) binary doesn't
/// ship, sqlx 0.9 fires `VersionMissing(n)` for the orphaned applied row.
/// The recovery path must back up the file without silently downgrading it.
///
/// This is the actual real-world "user downgraded the app" scenario. Note
/// that `MigrateError::VersionTooOld` is declared on the enum but is never
/// constructed by sqlx 0.9's `Migrator::run_direct`; the explicit
/// `VersionTooOld` arm in `db::connect` exists purely as defensive
/// programming for future sqlx releases.
#[tokio::test]
async fn version_missing_requires_explicit_recovery() {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-migration-downgrade-{id}.sqlite"));

    // First connect: applies both real migrations cleanly.
    let pool = db::connect(&path).await.expect("first connect").pool;
    // Simulate a newer app version that recorded a migration (version 999)
    // the current binary doesn't ship. sqlx's `validate_applied_migrations`
    // fires `VersionMissing(999)` on the next connect because 999 is in the
    // applied set but not in the embedded migration set.
    sqlx::query(
        r#"INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time)
           VALUES (999, 'future_migration_from_newer_app', '2026-01-01 00:00:00', 1, X'00', 0)"#,
    )
    .execute(&pool)
    .await
    .expect("insert fake future migration");
    pool.close().await;

    let recovery = match db::connect_for_startup(&path)
        .await
        .expect("inspect future migration")
    {
        db::DatabaseConnectOutcome::RecoveryRequired(recovery) => recovery,
        db::DatabaseConnectOutcome::Ready(_) => panic!("future migration must require recovery"),
    };
    assert_eq!(recovery.reason, "migration_missing");
    assert!(recovery.backup_verified);
    let backup_path = recovery.backup_path.expect("verified backup path");
    assert!(backup_path.is_file());
    assert!(path.is_file(), "the newer database must remain on disk");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    let _ = std::fs::remove_file(&backup_path);
}

/// After explicit recovery approval, a fresh database must contain the full schema.
#[tokio::test]
async fn explicit_reset_after_dirty_produces_clean_schema() {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-migration-rebuild-schema-{id}.sqlite"));

    let pool = db::connect(&path).await.expect("first connect").pool;
    sqlx::query("UPDATE _sqlx_migrations SET success = 0")
        .execute(&pool)
        .await
        .expect("flag dirty");
    pool.close().await;

    let recovery = match db::connect_for_startup(&path)
        .await
        .expect("inspect dirty migration")
    {
        db::DatabaseConnectOutcome::RecoveryRequired(recovery) => recovery,
        db::DatabaseConnectOutcome::Ready(_) => panic!("dirty migration must require recovery"),
    };
    assert!(recovery.backup_verified);
    db::reset_database_files(&path).expect("explicitly reset database files");
    let connection = db::connect(&path)
        .await
        .expect("connect after explicit reset");

    let tables = [
        "tasks",
        "task_files",
        "task_work_units",
        "task_events",
        "task_requests",
        "task_request_headers",
        "browser_messages",
        "task_credentials",
        "task_proxy_settings",
        "task_checksums",
        "classification_rules",
        "torrent_tasks",
        "torrent_runtime_snapshots",
        "hls_tasks",
        "hls_segments",
        "dash_tasks",
        "dash_segments",
        "metalink_tasks",
        "metalink_resources",
        "sftp_known_hosts",
        "settings",
    ];
    for table in &tables {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)",
        )
        .bind(table)
        .fetch_one(&connection.pool)
        .await
        .unwrap_or(false);
        assert!(
            exists,
            "table '{table}' should exist after rebuild, missing from clean schema"
        );
    }

    // The Phase 6 columns added by 002_metalink_health must also be present
    // on the rebuilt database, proving all migrations ran.
    let rows = sqlx::query("PRAGMA table_info(metalink_resources)")
        .fetch_all(&connection.pool)
        .await
        .expect("pragma");
    let columns: Vec<String> = rows
        .iter()
        .map(|row| row.get::<String, _>("name"))
        .collect();
    for expected in [
        "last_attempt_at",
        "cooldown_until",
        "avg_speed_bps",
        "supports_range",
    ] {
        assert!(
            columns.iter().any(|c| c == expected),
            "metalink_resources.{expected} missing after rebuild, actual: {columns:?}"
        );
    }

    connection.pool.close().await;
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    if let Some(backup_path) = recovery.backup_path {
        let _ = std::fs::remove_file(backup_path);
    }
}

// --- Migration 003: HLS track selection columns -------------------------------

/// Validates the two columns added by migration 003_hls_track_selection
/// are present on a fresh database and default to NULL:
///  - selected_audio_track_uris (TEXT, NULL on new rows)
///  - selected_subtitle_track_uris (TEXT, NULL on new rows)
///
/// Both columns store a JSON array of URI strings. NULL means "no selection"
/// (backwards-compatible with tasks created before F-6).
#[tokio::test]
async fn migration_003_adds_hls_track_selection_columns() {
    let pool = test_pool("hls-track-selection").await;

    let rows = sqlx::query("PRAGMA table_info(hls_tasks)")
        .fetch_all(&pool)
        .await
        .expect("pragma table_info(hls_tasks)");
    let columns: Vec<String> = rows
        .iter()
        .map(|row| row.get::<String, _>("name"))
        .collect();
    for expected in ["selected_audio_track_uris", "selected_subtitle_track_uris"] {
        assert!(
            columns.iter().any(|c| c == expected),
            "hls_tasks.{expected} missing after migration, actual: {columns:?}"
        );
    }

    // Insert a task record, then an hls_tasks row, and read the new columns
    // back to verify their NULL default behaviour.
    let now = now_iso();
    let task_id = "hls-track-selection-task".to_string();
    let task = TaskRecord {
        id: task_id.clone(),
        url: "https://example.com/stream.m3u8".to_string(),
        final_url: Some("https://example.com/stream.m3u8".to_string()),
        protocol: "hls".to_string(),
        task_kind: TaskKind::Manifest,
        file_name: "stream.mp4".to_string(),
        save_dir: std::env::temp_dir().to_string_lossy().to_string(),
        temp_path: None,
        final_path: None,
        total_size: 0,
        downloaded_bytes: 0,
        status: TaskStatus::Queued,
        etag: None,
        last_modified: None,
        content_type: None,
        supports_resume: false,
        supports_parallel: false,
        supports_multi_file: false,
        source_key: "hls-track-selection".to_string(),
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
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");

    let staging_dir = std::env::temp_dir()
        .join("hls-staging")
        .to_string_lossy()
        .to_string();
    let stamp = now_iso();
    sqlx::query(
        r#"INSERT INTO hls_tasks (task_id, input_url, media_url, staging_dir, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?)"#,
    )
    .bind(&task_id)
    .bind("https://example.com/stream.m3u8")
    .bind("https://example.com/stream.m3u8")
    .bind(&staging_dir)
    .bind(&stamp)
    .bind(&stamp)
    .execute(&pool)
    .await
    .expect("insert hls_task");

    let (audio_uris, subtitle_uris): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT selected_audio_track_uris, selected_subtitle_track_uris FROM hls_tasks WHERE task_id = ?",
    )
    .bind(&task_id)
    .fetch_one(&pool)
    .await
    .expect("fetch hls_task");
    assert!(
        audio_uris.is_none(),
        "selected_audio_track_uris default should be NULL"
    );
    assert!(
        subtitle_uris.is_none(),
        "selected_subtitle_track_uris default should be NULL"
    );

    pool.close().await;
}
