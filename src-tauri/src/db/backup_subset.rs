//! Safe-subset restore (feature proposal §3.7): merge selected row families
//! (tasks / rules / settings) from a verified backup snapshot into the LIVE
//! database without the whole-file swap.
//!
//! Everything is additive — `INSERT OR IGNORE` never modifies or deletes an
//! existing row — and the whole merge runs in one transaction on a dedicated
//! connection, so a failed attempt leaves the live database untouched.

use std::path::Path;

use sqlx::SqlitePool;

use crate::models::backup::{BackupSubsetRestoreResult, BackupSubsetSelection};

/// Tables keyed by `task_id REFERENCES tasks`, copied alongside their parent
/// task. The snapshot is migrated to the live schema version before this runs,
/// so the column shapes match exactly and `SELECT *` is stable. Mirrors the
/// `REFERENCES tasks` set in migrations 001 + 007.
const TASK_SATELLITE_TABLES: [&str; 18] = [
    "task_files",
    "task_work_units",
    "task_events",
    "task_requests",
    "task_request_headers",
    "task_request_profiles",
    "task_credentials",
    "task_proxy_settings",
    "task_checksums",
    "torrent_tasks",
    "torrent_runtime_snapshots",
    "hls_tasks",
    "hls_segments",
    "dash_tasks",
    "dash_segments",
    "metalink_tasks",
    "metalink_resources",
    "metalink_file_plans",
];

/// Settings row carrying the browser capture rules; restored with the rules
/// subset. Mirrors `SETTING_BROWSER_CAPTURE` in `commands/browser.rs` (kept
/// local to avoid a command-layer dependency from the DB layer).
const SETTING_BROWSER_CAPTURE: &str = "browser_capture_settings";

/// Merge selected row families from a verified snapshot into the live
/// database. `allowed_roots` must come from the live configuration (same
/// contract as [`super::backup::enforce_backup_path_policy`]): when the tasks
/// subset is selected, the snapshot's stored paths are checked against these
/// roots BEFORE any row lands in the live database, so a crafted backup
/// cannot smuggle write/delete targets past the SEC-02 policy through the
/// subset path.
pub async fn restore_subset(
    db_path: &Path,
    verified_db: &Path,
    selection: BackupSubsetSelection,
    allowed_roots: &[std::path::PathBuf],
) -> Result<BackupSubsetRestoreResult, String> {
    if !selection.tasks && !selection.rules && !selection.settings {
        return Err(engine_backup_error(
            "backup_subset_empty",
            "No restore subset was selected.",
        ));
    }
    if selection.tasks {
        super::backup::enforce_backup_path_policy(verified_db, allowed_roots).await?;
    }
    // A staged whole-file restore replaces the live database at next startup,
    // which would silently discard this merge. Refuse instead of racing the
    // pending swap.
    if super::backup::pending_restore_path(db_path).exists() {
        return Err(engine_backup_error(
            "backup_restore_pending",
            "A staged restore is waiting to be applied on restart; it must be applied or discarded first.",
        ));
    }
    let url = format!("sqlite://{}?mode=rw", db_path.display());
    let pool = SqlitePool::connect(&url)
        .await
        .map_err(|e| format!("Could not open the live database for subset restore: {e}"))?;
    let result = restore_subset_on_pool(&pool, verified_db, selection).await;
    pool.close().await;
    result
}

async fn restore_subset_on_pool(
    pool: &SqlitePool,
    verified_db: &Path,
    selection: BackupSubsetSelection,
) -> Result<BackupSubsetRestoreResult, String> {
    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| format!("Could not acquire the live database connection: {e}"))?;
    // ATTACH is prohibited inside a transaction, so it runs before BEGIN — on
    // the very connection the transaction will use (per-connection scope).
    sqlx::query("ATTACH DATABASE ? AS backup_subset")
        .bind(verified_db.to_string_lossy().to_string())
        .execute(&mut *conn)
        .await
        .map_err(|e| format!("Could not attach the backup snapshot: {e}"))?;

    // Explicit transaction instead of sqlx's `Acquire::begin` (which consumes
    // the connection, leaving no handle for the mandatory DETACH afterwards).
    if let Err(error) = sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await {
        detach_ignoring_errors(&mut conn).await;
        return Err(format!("Could not begin the subset restore: {error}"));
    }
    let mut result = BackupSubsetRestoreResult::default();
    match run_subset(&mut conn, selection, &mut result).await {
        Ok(()) => {}
        Err(error) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
            detach_ignoring_errors(&mut conn).await;
            return Err(error);
        }
    }
    if let Err(error) = sqlx::query("COMMIT").execute(&mut *conn).await {
        // The caller never sees the partial result when COMMIT fails.
        let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
        detach_ignoring_errors(&mut conn).await;
        return Err(format!("Could not commit the subset restore: {error}"));
    }
    detach_ignoring_errors(&mut conn).await;
    Ok(result)
}

async fn detach_ignoring_errors(conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>) {
    if let Err(error) = sqlx::query("DETACH DATABASE backup_subset")
        .execute(&mut **conn)
        .await
    {
        tracing::warn!(error = %error, "could not detach the backup snapshot");
    }
}

async fn run_subset(
    conn: &mut sqlx::SqliteConnection,
    selection: BackupSubsetSelection,
    result: &mut BackupSubsetRestoreResult,
) -> Result<(), String> {
    if selection.tasks {
        restore_tasks_subset(conn, result).await?;
    }
    if selection.rules {
        restore_rules_subset(conn, result).await?;
    }
    if selection.settings {
        restore_settings_subset(conn, result).await?;
    }
    Ok(())
}

/// Tasks subset: insert missing tasks verbatim, then normalize the ones that
/// cannot be trusted mid-flight to `paused`. `subset_new_tasks` is narrowed to
/// ids that ACTUALLY landed — `OR IGNORE` silently drops rows that collide
/// with the source-key or final-path unique indexes, and copying satellite
/// rows for a dropped task would violate its foreign key.
async fn restore_tasks_subset(
    conn: &mut sqlx::SqliteConnection,
    result: &mut BackupSubsetRestoreResult,
) -> Result<(), String> {
    sqlx::query("CREATE TEMP TABLE IF NOT EXISTS subset_new_tasks (id TEXT PRIMARY KEY)")
        .execute(&mut *conn)
        .await
        .map_err(|e| format!("Could not stage the subset task list: {e}"))?;
    sqlx::query("DELETE FROM temp.subset_new_tasks")
        .execute(&mut *conn)
        .await
        .map_err(|e| format!("Could not clear the subset task list: {e}"))?;
    sqlx::query(
        "INSERT INTO temp.subset_new_tasks (id)
         SELECT id FROM backup_subset.tasks WHERE id NOT IN (SELECT id FROM main.tasks)",
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| format!("Could not collect new task ids: {e}"))?;

    sqlx::query(
        "INSERT OR IGNORE INTO tasks SELECT * FROM backup_subset.tasks
         WHERE id IN (SELECT id FROM temp.subset_new_tasks)",
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| format!("Could not copy task rows: {e}"))?;

    // Keep only ids that were really inserted (unique-index collisions above).
    sqlx::query("DELETE FROM temp.subset_new_tasks WHERE id NOT IN (SELECT id FROM main.tasks)")
        .execute(&mut *conn)
        .await
        .map_err(|e| format!("Could not reconcile the subset task list: {e}"))?;

    // The status list is a compile-time constant, so the whole statement is a
    // literal — keeping the sqlx dynamic-SQL audit gate satisfied.
    result.tasks_normalized = sqlx::query(
        "UPDATE tasks SET status = 'paused', retry_after_at = NULL, error_message = NULL,
         error_code = NULL, recovery_actions = NULL, health_summary = NULL,
         speed_bps = 0, connection_count = 0, completed_at = NULL
         WHERE id IN (SELECT id FROM temp.subset_new_tasks)
           AND status NOT IN ('completed', 'failed', 'needs_attention')",
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| format!("Could not normalize restored task statuses: {e}"))?
    .rows_affected() as u32;

    result.tasks_inserted = sqlx::query_scalar("SELECT COUNT(*) FROM temp.subset_new_tasks")
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| format!("Could not count inserted tasks: {e}"))?;
    let backup_tasks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_subset.tasks")
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| format!("Could not count backup tasks: {e}"))?;
    result.tasks_skipped = (backup_tasks - result.tasks_inserted as i64).max(0) as u32;

    // SELECT * relies on the snapshot sharing the live schema version (it is
    // migrated forward by materialize_and_verify_backup_db before this runs).
    // Diagnostics tables with INTEGER AUTOINCREMENT ids (task_events,
    // task_requests) are resequenced by copying without their id column:
    // their ids are local sequence numbers, so a verbatim copy would collide
    // with the live sequence and be silently dropped by OR IGNORE.
    for table in TASK_SATELLITE_TABLES {
        copy_satellite_table(conn, table).await?;
    }
    Ok(())
}

async fn copy_satellite_table(
    conn: &mut sqlx::SqliteConnection,
    table: &'static str,
) -> Result<(), String> {
    let sql: &'static str = match table {
        "task_files" => "INSERT OR IGNORE INTO task_files SELECT * FROM backup_subset.task_files WHERE task_id IN (SELECT id FROM temp.subset_new_tasks)",
        "task_work_units" => "INSERT OR IGNORE INTO task_work_units SELECT * FROM backup_subset.task_work_units WHERE task_id IN (SELECT id FROM temp.subset_new_tasks)",
        "task_events" => "INSERT OR IGNORE INTO task_events (task_id, event_type, payload, created_at) SELECT task_id, event_type, payload, created_at FROM backup_subset.task_events WHERE task_id IN (SELECT id FROM temp.subset_new_tasks)",
        "task_requests" => "INSERT OR IGNORE INTO task_requests (task_id, method, url, range_header, if_range_header, status_code, etag, last_modified, content_length, error_message, retry_count, duration_ms, created_at) SELECT task_id, method, url, range_header, if_range_header, status_code, etag, last_modified, content_length, error_message, retry_count, duration_ms, created_at FROM backup_subset.task_requests WHERE task_id IN (SELECT id FROM temp.subset_new_tasks)",
        "task_request_profiles" => "INSERT OR IGNORE INTO task_request_profiles SELECT * FROM backup_subset.task_request_profiles WHERE task_id IN (SELECT id FROM temp.subset_new_tasks)",
        "task_request_headers" => "INSERT OR IGNORE INTO task_request_headers SELECT * FROM backup_subset.task_request_headers WHERE task_id IN (SELECT id FROM temp.subset_new_tasks)",
        "task_credentials" => "INSERT OR IGNORE INTO task_credentials SELECT * FROM backup_subset.task_credentials WHERE task_id IN (SELECT id FROM temp.subset_new_tasks)",
        "task_proxy_settings" => "INSERT OR IGNORE INTO task_proxy_settings SELECT * FROM backup_subset.task_proxy_settings WHERE task_id IN (SELECT id FROM temp.subset_new_tasks)",
        "task_checksums" => "INSERT OR IGNORE INTO task_checksums SELECT * FROM backup_subset.task_checksums WHERE task_id IN (SELECT id FROM temp.subset_new_tasks)",
        "torrent_tasks" => "INSERT OR IGNORE INTO torrent_tasks SELECT * FROM backup_subset.torrent_tasks WHERE task_id IN (SELECT id FROM temp.subset_new_tasks)",
        "torrent_runtime_snapshots" => "INSERT OR IGNORE INTO torrent_runtime_snapshots SELECT * FROM backup_subset.torrent_runtime_snapshots WHERE task_id IN (SELECT id FROM temp.subset_new_tasks)",
        "hls_tasks" => "INSERT OR IGNORE INTO hls_tasks SELECT * FROM backup_subset.hls_tasks WHERE task_id IN (SELECT id FROM temp.subset_new_tasks)",
        "hls_segments" => "INSERT OR IGNORE INTO hls_segments SELECT * FROM backup_subset.hls_segments WHERE task_id IN (SELECT id FROM temp.subset_new_tasks)",
        "dash_tasks" => "INSERT OR IGNORE INTO dash_tasks SELECT * FROM backup_subset.dash_tasks WHERE task_id IN (SELECT id FROM temp.subset_new_tasks)",
        "dash_segments" => "INSERT OR IGNORE INTO dash_segments SELECT * FROM backup_subset.dash_segments WHERE task_id IN (SELECT id FROM temp.subset_new_tasks)",
        "metalink_tasks" => "INSERT OR IGNORE INTO metalink_tasks SELECT * FROM backup_subset.metalink_tasks WHERE task_id IN (SELECT id FROM temp.subset_new_tasks)",
        "metalink_resources" => "INSERT OR IGNORE INTO metalink_resources SELECT * FROM backup_subset.metalink_resources WHERE task_id IN (SELECT id FROM temp.subset_new_tasks)",
        "metalink_file_plans" => "INSERT OR IGNORE INTO metalink_file_plans SELECT * FROM backup_subset.metalink_file_plans WHERE task_id IN (SELECT id FROM temp.subset_new_tasks)",
        _ => return Ok(()),
    };
    sqlx::query(sql)
        .execute(&mut *conn)
        .await
        .map_err(|e| format!("Could not copy backup rows into {table}: {e}"))?;
    Ok(())
}

/// Rules subset: classification rules are additive (id collisions skip), and
/// the browser capture rules travel with them because that row is rules-only
/// data (no secrets, no machine binding).
async fn restore_rules_subset(
    conn: &mut sqlx::SqliteConnection,
    result: &mut BackupSubsetRestoreResult,
) -> Result<(), String> {
    let live_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM classification_rules")
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| format!("Could not count live rules: {e}"))?;
    let backup_rules: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM backup_subset.classification_rules")
            .fetch_one(&mut *conn)
            .await
            .map_err(|e| format!("Could not count backup rules: {e}"))?;

    sqlx::query(
        "INSERT OR IGNORE INTO classification_rules SELECT * FROM backup_subset.classification_rules",
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| format!("Could not copy classification rules: {e}"))?;

    let live_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM classification_rules")
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| format!("Could not recount live rules: {e}"))?;
    result.rules_inserted = (live_after - live_before).max(0) as u32;
    result.rules_skipped = (backup_rules - result.rules_inserted as i64).max(0) as u32;

    sqlx::query(
        "INSERT OR REPLACE INTO settings(key, value)
         SELECT key, value FROM backup_subset.settings WHERE key = ?",
    )
    .bind(SETTING_BROWSER_CAPTURE)
    .execute(&mut *conn)
    .await
    .map_err(|e| format!("Could not restore the capture rules row: {e}"))?;
    Ok(())
}

/// Settings subset: replace every settings row with the backup's, then run
/// the same machine-bound scrub the whole-file restore applies, so the
/// proxy-password flag, ffmpeg path, and completion action land in a state
/// that is safe on THIS machine (SEC-09) even though the values came from
/// another database.
async fn restore_settings_subset(
    conn: &mut sqlx::SqliteConnection,
    result: &mut BackupSubsetRestoreResult,
) -> Result<(), String> {
    let backup_keys: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_subset.settings")
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| format!("Could not count backup settings: {e}"))?;
    sqlx::query(
        "INSERT OR REPLACE INTO settings(key, value) SELECT key, value FROM backup_subset.settings",
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| format!("Could not restore settings rows: {e}"))?;
    let proxy_password_present =
        crate::proxy::load_proxy_password().is_ok_and(|value| value.is_some());
    super::backup::run_restore_scrub_core(&mut *conn, proxy_password_present).await?;
    // Every backup settings row is written (REPLACE), so the count is the
    // backup's key count — the scrub afterwards rewrites three of them to
    // machine-bound values.
    result.settings_replaced = backup_keys as u32;
    Ok(())
}

fn engine_backup_error(code: &str, message: impl Into<String>) -> String {
    crate::models::AppErrorPayload::new(code, message, false, vec!["check_url"]).command_error()
}
