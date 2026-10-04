//! IPC models for the Integrity Passport (feature proposal §2.4).
//!
//! The passport separates "download finished" from "output trustworthy":
//! every field is either a recorded fact (checksums, remote validators) or an
//! honest unknown — event-derived timestamps can be pruned by retention, and
//! a missing remote checksum must surface as `not_provided`, never as trust.
//! Labels are resolved on the frontend from typed tables, so no pre-rendered
//! sentences cross the IPC boundary.

use serde::Serialize;
use specta::Type;

use super::task::{HashVerificationStatus, TaskKind, TaskStatus};

/// Verification outcome of one checksum entry in the passport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PassportChecksumState {
    Verified,
    Failed,
    Pending,
    /// No checksum was ever configured for this task. The passport must say
    /// so explicitly instead of leaving the section blank (honesty rule).
    NotProvided,
}

/// Proof state of post-download staging cleanup for the task output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PassportStagingCleanup {
    /// All known output paths exist on disk and no temp/staging residue.
    Complete,
    /// Output present but temp/staging residue remains on disk.
    Incomplete,
    /// At least one known output path no longer exists on disk.
    MissingOutput,
    /// Task is not completed — cleanup proof does not apply yet.
    NotApplicable,
}

/// Remote validator evidence the download relied on. Tokens are stable
/// identifiers; the frontend maps them to localized labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RemoteValidatorKind {
    Etag,
    LastModified,
    Range,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PassportChecksum {
    pub algorithm: String,
    pub status: HashVerificationStatus,
    pub actual_hash: Option<String>,
    pub verified_at: Option<String>,
    pub is_primary: bool,
    pub weak: bool,
    pub source_kind: String,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityPassport {
    pub task_id: String,
    pub file_name: String,
    /// Stored task URL — already credential-sanitized at creation (FUN-01).
    pub source_url: String,
    pub final_url: Option<String>,
    pub protocol: String,
    pub task_kind: TaskKind,
    pub status: TaskStatus,
    /// Specta cannot map u64, so byte counts cross IPC as strings
    /// (models/storage.rs convention). `None` when the size is unknown.
    pub total_bytes: Option<String>,
    pub downloaded_bytes: Option<String>,
    pub created_at: String,
    /// First `started` event time; `None` when pruned by event retention.
    pub started_at: Option<String>,
    /// Durable completion time; legacy rows without the migration stay `None`.
    pub completed_at: Option<String>,
    /// Number of observed resumes (`resumed` events).
    pub resume_count: u32,
    /// Sum of per-segment retry counters — segment-level errors that were
    /// recovered in-flight. This is the honest derivable stand-in for the
    /// proposal's "checkpoints" stat; checkpoint flushes are not counted.
    pub segment_retries: u32,
    pub supports_resume: bool,
    pub remote_validators: Vec<RemoteValidatorKind>,
    pub checksums: Vec<PassportChecksum>,
    pub checksum_state: PassportChecksumState,
    pub staging_cleanup: PassportStagingCleanup,
    pub final_path: Option<String>,
}
