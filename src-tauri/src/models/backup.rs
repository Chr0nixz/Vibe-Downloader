//! IPC models for the Backup & Migration Center (feature proposal §3.7).
//!
//! Every struct is data-only: counts, stable codes, and paths. Any sentence a
//! user sees is composed in the frontend from typed i18n tables, so no
//! pre-rendered English crosses the IPC boundary (UX-11 raw-message rule).

use serde::{Deserialize, Serialize};
use specta::Type;

/// Row counts describing what a backup — or the live database — contains.
/// Produced by the same counter for both sides, so the export preview and the
/// restore preview always use identical semantics.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupContents {
    pub tasks_total: u32,
    pub tasks_completed: u32,
    pub tasks_failed: u32,
    pub classification_rules: u32,
    pub site_rules: u32,
    pub tasks_with_checksums: u32,
    pub tasks_with_credentials: u32,
    pub tasks_with_request_headers: u32,
    pub settings_keys: u32,
    pub task_events: u32,
}

/// SEC-02 path-policy scan of a backup in reporting form. Restore itself
/// still fails closed on the first offending path; this summary only feeds
/// the pre-restore check panel and the remap suggestion.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupPathPolicySummary {
    pub violation_count: u32,
    /// Bounded sample of offending paths (`table.column of row …`).
    pub sample_violations: Vec<String>,
    /// Distinct task save dirs outside the allowed roots (bounded), so the UI
    /// can explain *which* folders a migration remap would relocate.
    pub offending_save_dirs: Vec<String>,
}

/// Free space on the volume hosting the live database versus what a restore
/// needs (staged pending file + pre-restore snapshot + WAL headroom).
/// `free_bytes` is `None` when the platform query is unavailable — the check
/// degrades to "unknown", never to "failed".
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupDiskCheck {
    /// Byte counts are strings across the IPC boundary (Specta forbids u64);
    /// the frontend formats them through the shared byte formatter.
    pub free_bytes: Option<String>,
    pub required_bytes: String,
}

/// What the pre-restore scrub will touch, read from the backup itself, so the
/// user sees the whitelist consequence *before* committing (§3.7).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupSettingsPreview {
    pub ffmpeg_configured: bool,
    pub completion_action: String,
    pub proxy_password_saved: bool,
    pub default_save_dir: String,
}

/// Which safe subsets to merge from a backup. Each flag is independent;
/// selecting none is a caller error rejected with `backup_invalid_remap_root`
/// family validation (`backup_subset_empty`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupSubsetSelection {
    pub tasks: bool,
    pub rules: bool,
    pub settings: bool,
}

/// Outcome counts of a partial (subset) restore. Everything is additive:
/// existing rows are never modified or deleted, so a failed attempt leaves
/// the live database intact.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupSubsetRestoreResult {
    pub tasks_inserted: u32,
    pub tasks_skipped: u32,
    /// Active statuses from the backup (queued/downloading/retrying/…) that
    /// were normalized to `paused` because their temp state cannot be trusted
    /// on this machine.
    pub tasks_normalized: u32,
    pub rules_inserted: u32,
    pub rules_skipped: u32,
    pub settings_replaced: u32,
}

/// Post-restore "what to reconfigure" report, written next to the database
/// after a whole-file restore is applied at startup. Data fields only; the
/// page renders them through typed i18n keys.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    /// Schema version of the restored database after forward migration.
    pub schema_version: String,
    /// RFC 3339 timestamp of when the pending file was applied.
    pub restored_at: String,
    pub backup_created_at: Option<String>,
    /// Snapshot of the pre-restore database kept for manual rollback.
    pub pre_restore_backup_path: Option<String>,
    pub tasks_with_credentials: u32,
    pub tasks_with_per_task_proxy: u32,
    /// The backup claimed a global proxy password but this machine's keyring
    /// has none — the password must be re-entered in settings.
    pub global_proxy_needs_reentry: bool,
    /// The backup configured ffmpeg; the SEC-09 scrub cleared the path.
    pub ffmpeg_was_configured: bool,
    /// The backup had a non-notify completion action; the scrub reset it.
    pub completion_action_reset: bool,
    /// Distinct task save dirs that do not exist on this machine (bounded).
    pub missing_save_dirs: Vec<String>,
    pub missing_save_dirs_total: u32,
}
