//! IPC models for the Storage & Cleanup Center (feature proposal §3.2).
//!
//! Sizes are serialized as strings because Specta cannot map `u64` to the
//! JS side (same restriction as `DiskSpaceInfo`). Every item carries an
//! opaque id that the frontend echoes back verbatim on cleanup — the
//! frontend never builds or displays artifact paths.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Category of a temporary download artifact. Mirrors the classification in
/// `download::artifacts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    TempFile,
    LegacyTempFile,
    StagingDir,
    PublishStaging,
    MetalinkPart,
    DhtState,
}

/// Why an artifact is (or is not) reclaimable. Stable code, mapped to i18n in
/// the frontend — never a pre-rendered sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactReason {
    /// No task row claims it.
    NoOwner,
    /// The only claiming task is completed; the final file was published.
    OwnerCompleted,
    /// BT DHT state file older than the stale threshold.
    DhtStale,
    /// A live, non-completed task still needs the bytes for resume/retry.
    OwnerResumable,
}

/// One artifact listed by a scan.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageArtifactItem {
    /// Opaque identity (the absolute path). Echoed back on cleanup; the
    /// backend re-validates classification before deleting.
    pub id: String,
    pub kind: ArtifactKind,
    pub save_dir: String,
    pub file_name: String,
    pub bytes: String,
    /// RFC 3339 timestamp, if the platform reports one.
    pub modified_at: Option<String>,
    pub reclaimable: bool,
    pub reason: ArtifactReason,
    pub owner_task_id: Option<String>,
    /// Owner task display name for the per-task view, when the owner exists.
    pub task_file_name: Option<String>,
    /// Owner protocol, when the owner exists (per-task view context).
    pub owner_protocol: Option<String>,
}

/// Disk usage overview for one save directory.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SaveDirOverview {
    pub path: String,
    pub total_bytes: String,
    pub available_bytes: String,
    /// Sum of reclaimable artifact bytes under this dir.
    pub reclaimable_bytes: String,
    /// Sum of bytes still needed by live (non-completed) task artifacts.
    pub resumable_bytes: String,
    /// Available space divided by the average completed task size in this
    /// dir; `None` when no completed task provides an average.
    pub estimated_completable_tasks: Option<String>,
    /// The directory walk hit the entry cap; numbers are lower bounds.
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageScanResult {
    pub scan_id: String,
    pub scanned_at: String,
    pub dirs: Vec<SaveDirOverview>,
    pub items: Vec<StorageArtifactItem>,
}

/// Which reclaimable items a cleanup run targets. Appears both as command
/// input and inside the result payload, so it derives both directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CleanupMode {
    /// Artifacts with no owning task row.
    Orphans,
    /// Artifacts whose only owner is a completed task.
    CompletedLeftovers,
    /// Both of the above.
    AllReclaimable,
    /// Explicit item ids from a previous scan (re-validated before delete).
    Selected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CleanupOutcome {
    Removed,
    Skipped,
    Failed,
}

/// Per-item result of a cleanup run, so partial success is never reported as
/// full success.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CleanupItemOutcome {
    pub item_id: String,
    pub outcome: CleanupOutcome,
    pub bytes: String,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageCleanupResult {
    pub request_id: String,
    pub mode: CleanupMode,
    pub removed_count: u32,
    pub skipped_count: u32,
    pub failed_count: u32,
    pub reclaimed_bytes: String,
    pub outcomes: Vec<CleanupItemOutcome>,
    /// Set for `cleanup_task_temp_files`: the task will restart from zero on
    /// its next start.
    pub resume_discarded: bool,
}

/// Persisted record of one sweep (startup or manual), shown on the page.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageSweepRecord {
    pub id: String,
    pub started_at: String,
    pub finished_at: String,
    pub mode: String,
    pub removed_count: u32,
    pub failed_count: u32,
    pub reclaimed_bytes: String,
}
