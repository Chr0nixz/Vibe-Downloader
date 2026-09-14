//! Post-restore "what to reconfigure" report (feature proposal §3.7).
//!
//! The report lives in a sidecar JSON next to the database, NOT inside the
//! database: a whole-file restore replaces the database wholesale, so anything
//! written into it before the swap is gone (the FUN-26 lesson). The sidecar
//! survives the swap and is surfaced by the Backup Center on next launch.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::backup::RestoreScrubObservation;
use crate::models::backup::RestoreReport;

const RESTORE_REPORT_SUFFIX: &str = ".restore-report.json";
const STAGING_META_SUFFIX: &str = ".meta.json";
/// The report stays bounded: at most ten missing dirs are named, the total is
/// counted separately.
const MAX_MISSING_DIRS: usize = 10;

pub fn restore_report_path(db_path: &Path) -> PathBuf {
    let mut path = db_path.as_os_str().to_owned();
    path.push(RESTORE_REPORT_SUFFIX);
    PathBuf::from(path)
}

fn staging_meta_path(pending: &Path) -> PathBuf {
    let mut path = pending.as_os_str().to_owned();
    path.push(STAGING_META_SUFFIX);
    PathBuf::from(path)
}

/// Written by the restore command at staging time; consumed when the pending
/// file is applied at startup. Without it (e.g. a pending file staged by an
/// older build) the report is still produced, just without rollback info.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreStagingMeta {
    pub pre_restore_backup_path: String,
    pub backup_created_at: String,
    pub schema_version: i64,
}

pub fn write_staging_meta(pending: &Path, meta: &RestoreStagingMeta) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(meta)
        .map_err(|e| format!("Could not serialize restore staging meta: {e}"))?;
    std::fs::write(staging_meta_path(pending), json)
        .map_err(|e| format!("Could not write restore staging meta: {e}"))
}

/// Read and consume the staging meta. Best-effort: a missing or malformed
/// meta degrades the report, never blocks the restore apply.
pub fn take_staging_meta(pending: &Path) -> Option<RestoreStagingMeta> {
    let path = staging_meta_path(pending);
    let bytes = std::fs::read(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    serde_json::from_slice(&bytes).ok()
}

/// Build and persist the report for the freshly swapped database. The
/// database at `db_path` is already the restored one when this runs.
pub(crate) async fn write_report_after_swap(
    db_path: &Path,
    scrub: Option<&RestoreScrubObservation>,
) -> Result<(), String> {
    let url = format!("sqlite://{}?mode=ro", db_path.display());
    let report_pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .map_err(|e| format!("Could not open the restored database for the report: {e}"))?;
    let built = build_report(&report_pool, db_path, scrub).await;
    report_pool.close().await;
    let report = built?;

    let json = serde_json::to_vec_pretty(&report)
        .map_err(|e| format!("Could not serialize restore report: {e}"))?;
    std::fs::write(restore_report_path(db_path), json)
        .map_err(|e| format!("Could not write restore report: {e}"))
}

async fn build_report(
    pool: &SqlitePool,
    db_path: &Path,
    scrub: Option<&RestoreScrubObservation>,
) -> Result<RestoreReport, String> {
    let schema_version = super::backup::current_schema_version(pool)
        .await?
        .to_string();
    let meta = take_staging_meta(&super::backup::pending_restore_path(db_path));
    async fn count_distinct(pool: &SqlitePool, sql: &'static str) -> Result<u32, String> {
        let (value,): (i64,) = sqlx::query_as(sql)
            .fetch_one(pool)
            .await
            .map_err(|e| format!("Could not build restore report: {e}"))?;
        Ok(value as u32)
    }
    let tasks_with_credentials =
        count_distinct(pool, "SELECT COUNT(DISTINCT task_id) FROM task_credentials").await?;
    let tasks_with_per_task_proxy = count_distinct(
        pool,
        "SELECT COUNT(DISTINCT task_id) FROM task_proxy_settings",
    )
    .await?;

    let save_dirs = super::backup_contents::distinct_save_dirs(pool).await?;
    let missing: Vec<String> = save_dirs
        .iter()
        .filter(|dir| !Path::new(&dir).exists())
        .cloned()
        .collect();
    let missing_save_dirs_total = missing.len() as u32;

    // The scrub corrected `proxy_password_saved` to local keyring reality; the
    // report explains the consequence when the backup claimed a password this
    // machine does not have.
    let proxy_present = crate::proxy::load_proxy_password().is_ok_and(|value| value.is_some());
    let global_proxy_needs_reentry = scrub
        .and_then(|obs| obs.proxy_password_saved_in_backup)
        .unwrap_or(false)
        && !proxy_present;

    Ok(RestoreReport {
        schema_version,
        restored_at: chrono::Utc::now().to_rfc3339(),
        backup_created_at: meta.as_ref().map(|m| m.backup_created_at.clone()),
        pre_restore_backup_path: meta.as_ref().map(|m| m.pre_restore_backup_path.clone()),
        tasks_with_credentials,
        tasks_with_per_task_proxy,
        global_proxy_needs_reentry,
        ffmpeg_was_configured: scrub.map(|obs| obs.ffmpeg_was_configured).unwrap_or(false),
        completion_action_reset: scrub
            .and_then(|obs| obs.completion_action_was.clone())
            .is_some_and(|value| value != "notify"),
        missing_save_dirs: missing.into_iter().take(MAX_MISSING_DIRS).collect(),
        missing_save_dirs_total,
    })
}

/// Read the last report, if one has not been dismissed. Malformed sidecars are
/// treated as absent rather than surfaced as errors.
pub fn read_restore_report(db_path: &Path) -> Option<RestoreReport> {
    let bytes = std::fs::read(restore_report_path(db_path)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Delete the report. Returns whether one was actually removed.
pub fn dismiss_restore_report(db_path: &Path) -> bool {
    let path = restore_report_path(db_path);
    if path.exists() {
        std::fs::remove_file(path).is_ok()
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirror the integration-test temp-dir convention (no tempfile dep).
    fn scratch_dir(label: &str) -> PathBuf {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("vibe-report-{label}-{id}"));
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn report_round_trip_through_sidecar() {
        let dir = scratch_dir("roundtrip");
        let db_path = dir.join("vibe.db");
        std::fs::write(&db_path, b"stub").expect("stub db");
        assert!(read_restore_report(&db_path).is_none());

        let report = RestoreReport {
            schema_version: "9".into(),
            restored_at: "2026-09-13T00:00:00Z".into(),
            backup_created_at: Some("2026-09-01T00:00:00Z".into()),
            pre_restore_backup_path: Some("/data/vibe.db.bak-1".into()),
            tasks_with_credentials: 3,
            tasks_with_per_task_proxy: 1,
            global_proxy_needs_reentry: true,
            ffmpeg_was_configured: true,
            completion_action_reset: true,
            missing_save_dirs: vec!["/old/Downloads".into()],
            missing_save_dirs_total: 1,
        };
        let json = serde_json::to_vec_pretty(&report).expect("serialize");
        std::fs::write(restore_report_path(&db_path), json).expect("write");
        let read = read_restore_report(&db_path).expect("report present");
        assert_eq!(read.tasks_with_credentials, 3);
        assert!(read.global_proxy_needs_reentry);

        assert!(dismiss_restore_report(&db_path));
        assert!(!dismiss_restore_report(&db_path));
        assert!(read_restore_report(&db_path).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn staging_meta_is_consumed_once() {
        let dir = scratch_dir("meta");
        let pending = dir.join("vibe.db.vibe-restore-pending");
        let meta = RestoreStagingMeta {
            pre_restore_backup_path: "/data/vibe.db.bak-1".into(),
            backup_created_at: "2026-09-01T00:00:00Z".into(),
            schema_version: 8,
        };
        write_staging_meta(&pending, &meta).expect("write meta");
        let taken = take_staging_meta(&pending).expect("meta present");
        assert_eq!(taken.schema_version, 8);
        assert!(take_staging_meta(&pending).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
