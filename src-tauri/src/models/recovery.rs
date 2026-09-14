//! IPC models for the Recovery Center (feature proposal §3.3).
//!
//! Recovery history rows are persisted outcomes keyed by stable identifiers:
//! `action` mirrors `RecoveryAction::as_str()` and `source` names the surface
//! that performed the resolution, so the frontend maps both through typed
//! i18n tables — no pre-rendered sentences cross the IPC boundary.

use serde::{Deserialize, Serialize};
use specta::Type;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// One persisted recovery resolution shown in the recovery history.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryHistoryRecord {
    pub id: String,
    pub task_id: String,
    /// Task display name captured at resolution time, when the row existed.
    pub task_file_name: Option<String>,
    /// Stable recovery action identifier (`RecoveryAction::as_str()`).
    pub action: String,
    /// Which surface performed it (`recovery_center`, `manual`, `auto`).
    pub source: String,
    /// Stable error code active at resolution time, when known.
    pub error_code: Option<String>,
    /// RFC 3339 timestamp.
    pub created_at: String,
}

/// Outcome counts for a batch attention resolution. Per-task failures are
/// logged with their error and counted, never aborting the batch.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BulkRecoveryResult {
    pub succeeded: u32,
    pub skipped: u32,
    pub failed: u32,
}

/// Non-destructive re-queue actions allowed in a batch resolution. Restart
/// (delete temp artifacts + re-probe) is intentionally excluded from bulk
/// use: it stays per-task behind a hard confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BulkRecoveryAction {
    Retry,
    RetryLater,
}

impl BulkRecoveryAction {
    /// Matches the `RecoveryAction::as_str()` spellings so history rows and
    /// frontend label tables stay aligned.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Retry => "retry",
            Self::RetryLater => "retry_later",
        }
    }
}

/// Replacement credentials for a task that failed with an auth error.
/// SEC-05: the plaintext fields are wiped when this value is dropped.
#[derive(Debug, Clone, Deserialize, Type, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTaskCredentialsInput {
    pub task_id: String,
    pub username: String,
    pub password: String,
    pub private_key_data: Option<String>,
    pub private_key_passphrase: Option<String>,
}
