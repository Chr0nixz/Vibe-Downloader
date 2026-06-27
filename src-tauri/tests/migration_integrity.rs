use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::{AssertSqlSafe, Row};
use tauri_app_lib::db;

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
    // merged into a single `001_init.sql`. A fresh install records exactly one
    // migration row.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .expect("count migrations");
    assert_eq!(count, 1, "expected exactly 1 baseline migration, got {count}");

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
        count, 1,
        "expected exactly 1 migration on reconnect, got {count}"
    );
    pool2.close().await;

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
}
