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
    // merged into a single `001_init.sql`, followed by `002_metalink_health.sql`
    // (Phase 6 parallel-mirror health tracking). A fresh install records
    // exactly two migration rows.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .expect("count migrations");
    assert_eq!(count, 2, "expected exactly 2 migrations (baseline + metalink health), got {count}");

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
        assert!(
            exists,
            "table '{table}' should exist after migration"
        );
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
async fn dedup_unique_index_exists() {
    let pool = test_pool("dedup").await;

    let indexes = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='tasks'",
    )
    .fetch_all(&pool)
    .await
    .expect("list task indexes");
    let index_names: Vec<String> = indexes
        .iter()
        .map(|row| row.get::<String, _>("name"))
        .collect();

    assert!(
        index_names
            .iter()
            .any(|n| n == "idx_tasks_source_key_active"),
        "dedup unique index 'idx_tasks_source_key_active' should exist, got: {index_names:?}"
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
    let columns: Vec<String> = rows.iter().map(|row| row.get::<String, _>("name")).collect();
    for expected in ["last_attempt_at", "cooldown_until", "avg_speed_bps", "supports_range"] {
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

    let (supports_range, avg_speed, last_attempt, cooldown): (i64, i64, Option<String>, Option<String>) =
        sqlx::query_as(
            "SELECT supports_range, avg_speed_bps, last_attempt_at, cooldown_until \
             FROM metalink_resources WHERE id = 'metalink-health-resource'",
        )
        .fetch_one(&pool)
        .await
        .expect("fetch defaults");
    assert_eq!(supports_range, 1, "supports_range default should be 1 (true)");
    assert_eq!(avg_speed, 0, "avg_speed_bps default should be 0");
    assert!(last_attempt.is_none(), "last_attempt_at default should be NULL");
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
        count, 2,
        "expected exactly 2 migrations on reconnect (baseline + metalink health), got {count}"
    );
    pool2.close().await;

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
}

// --- A-1: Dirty / VersionTooOld recovery -------------------------------------

/// Validates the A-1 recovery flow: when a previous migration run was
/// interrupted (sqlx leaves `_sqlx_migrations` flagged dirty), the next
/// `db::connect` must:
///
/// 1. Detect the dirty state via `MigrateError::Dirty`.
/// 2. Back up the corrupted database file alongside the original path.
/// 3. Drop the file and re-run all migrations from scratch.
/// 4. Return a `DbConnection` with `data_was_reset == true` and the backup
///    path surfaced so the startup dialog can reveal it to the user.
#[tokio::test]
async fn rebuilds_for_dirty_migration() {
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

    // Second connect must trigger the rebuild path.
    let connection = db::connect(&path)
        .await
        .expect("rebuild after dirty migration");
    assert!(
        connection.data_was_reset,
        "Dirty migration should have triggered a rebuild"
    );
    let backup_path = connection
        .backup_path
        .as_ref()
        .expect("backup path should be surfaced after rebuild");
    assert!(
        backup_path.exists(),
        "backup file should exist on disk at {}",
        backup_path.display()
    );

    // The rebuilt database must have a clean migration history (both rows
    // marked success=1).
    let dirty_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 0")
            .fetch_one(&connection.pool)
            .await
            .expect("count dirty");
    assert_eq!(
        dirty_count, 0,
        "no migration row should be flagged dirty after rebuild"
    );

    connection.pool.close().await;
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    let _ = std::fs::remove_file(backup_path);
}

/// Validates the A-1 downgrade recovery path. When a previous (newer) version
/// of the app recorded a migration that the current (older) binary doesn't
/// ship, sqlx 0.9 fires `VersionMissing(n)` for the orphaned applied row.
/// Our recovery path treats this the same as `Dirty`: back up the file, drop
/// it, and re-run all migrations from scratch — surfacing `data_was_reset`
/// so the startup dialog can offer to open the backup folder.
///
/// This is the actual real-world "user downgraded the app" scenario. Note
/// that `MigrateError::VersionTooOld` is declared on the enum but is never
/// constructed by sqlx 0.9's `Migrator::run_direct`; the explicit
/// `VersionTooOld` arm in `db::connect` exists purely as defensive
/// programming for future sqlx releases.
#[tokio::test]
async fn rebuilds_for_version_missing_after_downgrade() {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir()
        .join(format!("vibe-migration-downgrade-{id}.sqlite"));

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

    // Second connect must trigger the rebuild path. `DbConnection` doesn't
    // implement `Debug` (it wraps a `SqlitePool`), so we match on the
    // `Ok` variant manually rather than using `expect`.
    let connection = match db::connect(&path).await {
        Ok(connection) => connection,
        Err(message) => panic!(
            "VersionMissing should trigger a rebuild, not surface an error. Got: {message}"
        ),
    };
    assert!(
        connection.data_was_reset,
        "VersionMissing should have triggered a rebuild"
    );
    let backup_path = connection
        .backup_path
        .as_ref()
        .expect("backup path should be surfaced after rebuild");
    assert!(
        backup_path.exists(),
        "backup file should exist on disk at {}",
        backup_path.display()
    );

    // The rebuilt database must have a clean migration history: only the
    // two real migrations (versions 1 and 2), no orphaned version 999 row.
    let migrations: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&connection.pool)
            .await
            .expect("list migrations");
    assert_eq!(
        migrations,
        vec![1, 2],
        "rebuilt database should only contain the embedded migration versions, got: {migrations:?}"
    );

    connection.pool.close().await;
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    let _ = std::fs::remove_file(backup_path);
}

/// Validates that after a Dirty-triggered rebuild, the rebuilt database
/// still contains every key table from the baseline schema — the rebuild
/// path doesn't silently skip any migration.
#[tokio::test]
async fn rebuild_after_dirty_produces_clean_schema() {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir()
        .join(format!("vibe-migration-rebuild-schema-{id}.sqlite"));

    let pool = db::connect(&path).await.expect("first connect").pool;
    sqlx::query("UPDATE _sqlx_migrations SET success = 0")
        .execute(&pool)
        .await
        .expect("flag dirty");
    pool.close().await;

    let connection = db::connect(&path)
        .await
        .expect("rebuild after dirty migration");
    assert!(connection.data_was_reset);

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
    // on the rebuilt database, proving both migrations ran.
    let rows = sqlx::query("PRAGMA table_info(metalink_resources)")
        .fetch_all(&connection.pool)
        .await
        .expect("pragma");
    let columns: Vec<String> = rows.iter().map(|row| row.get::<String, _>("name")).collect();
    for expected in ["last_attempt_at", "cooldown_until", "avg_speed_bps", "supports_range"] {
        assert!(
            columns.iter().any(|c| c == expected),
            "metalink_resources.{expected} missing after rebuild, actual: {columns:?}"
        );
    }

    connection.pool.close().await;
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
}
