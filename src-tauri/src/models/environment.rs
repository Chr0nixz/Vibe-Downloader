//! Environment health-check models for Settings → Environment.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::models::BrowserKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentHealthStatus {
    Ok,
    Warn,
    Error,
    Unknown,
}

impl EnvironmentHealthStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Unknown => "unknown",
        }
    }
}

/// Suggested recovery action. Frontend localizes labels by `kind`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentFixAction {
    pub kind: EnvironmentFixKind,
    /// Optional browser target for `install_native_host`.
    pub browser: Option<BrowserKind>,
    /// Path kind for `open_path`: `save_dir` | `data` | `log`.
    pub path_kind: Option<String>,
    /// Settings section id for `focus_setting`.
    pub section: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentFixKind {
    InstallNativeHost,
    OpenPath,
    FocusSetting,
    ExportBackup,
    CheckForUpdate,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentHealthItem {
    /// Stable id: `native_host` | `browser` | `ffmpeg` | `proxy` | `save_dir` | `disk` | `database`.
    pub id: String,
    pub status: EnvironmentHealthStatus,
    /// English machine-facing summary for the copyable report.
    pub summary: String,
    pub detail: Option<String>,
    pub suggested_actions: Vec<EnvironmentFixAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentHealthReport {
    /// Unix epoch milliseconds as a decimal string (avoids Specta BigInt ban).
    pub checked_at_ms: String,
    pub app_version: String,
    pub platform: String,
    pub items: Vec<EnvironmentHealthItem>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentFixInput {
    pub kind: EnvironmentFixKind,
    pub browser: Option<BrowserKind>,
    pub path_kind: Option<String>,
    pub section: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentFixResult {
    pub ok: bool,
    pub message: String,
    /// When set, the frontend should scroll/expand this Settings section.
    pub focus_section: Option<String>,
    /// True when the caller should re-run `get_environment_health`.
    pub refresh: bool,
}
