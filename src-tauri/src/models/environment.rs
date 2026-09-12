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

/// Stable code for a piece of environment copy. The frontend maps each variant
/// to an `environment.<code>` i18n key, so the backend never ships user-facing
/// English. Do not rename a variant without updating every locale bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum EnvironmentTextCode {
    /// Opaque value kept verbatim: a filesystem path, a version, a browser
    /// handoff error, or any other raw backend string. Per `UX-11` these stay in
    /// diagnostics and are deliberately not translated.
    Raw,
    NativeHostReady,
    NativeHostMissing,
    BrowserNoneDetected,
    BrowserNeedsNativeHost,
    BrowserMissingManifests,
    BrowserReady,
    BrowserBridgeOffline,
    FfmpegReady,
    FfmpegUnprobeable,
    FfmpegMissing,
    ProxyDisabled,
    ProxySystemUnprobeable,
    ProxyHandshakeOk,
    ProxyHandshakeFailed,
    SaveDirWritable,
    SaveDirNotWritable,
    DiskOk,
    DiskLow,
    DiskCritical,
    DiskUnclassified,
    DiskQueryFailed,
    DiskUsage,
    DatabaseIntegrityFailed,
    DatabaseBackedUp,
    DatabaseNoBackup,
    BridgeConnected,
    BridgeListening,
    BridgeUnavailable,
    RecentHandoffErrors,
    ProxySystemInherits,
    FixNativeHostMissing,
    FixNoBrowsersToInstall,
    FixManifestsInstalled,
    FixOpenedPath,
    FixFocusSection,
    FixChooseBackupDestination,
    FixCheckFromUpdater,
}

/// Interpolation values for an [`EnvironmentText`]. Every field is optional so a
/// single shape covers all codes; the frontend passes them straight to i18next.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentTextParams {
    pub count: Option<u32>,
    pub names: Option<String>,
    pub url: Option<String>,
    pub errors: Option<String>,
    pub path: Option<String>,
    /// Settings section id, e.g. `browser-integration`. An internal id, not copy.
    pub section: Option<String>,
    pub available: Option<String>,
    pub total: Option<String>,
}

/// One localizable fragment of environment copy. `English` is the text the
/// copied diagnostics report prints; the app localizes by `code` and falls back
/// to `english` when the frontend does not know the code yet.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentText {
    pub code: EnvironmentTextCode,
    pub params: EnvironmentTextParams,
    /// English source text. Also the value shown for [`EnvironmentTextCode::Raw`].
    pub english: String,
}

impl EnvironmentText {
    /// A localized-capable fragment.
    pub fn new(code: EnvironmentTextCode, english: impl Into<String>) -> Self {
        Self {
            code,
            params: EnvironmentTextParams::default(),
            english: english.into(),
        }
    }

    /// A localized-capable fragment carrying interpolation values.
    pub fn with_params(
        code: EnvironmentTextCode,
        english: impl Into<String>,
        params: EnvironmentTextParams,
    ) -> Self {
        Self {
            code,
            params,
            english: english.into(),
        }
    }

    /// An opaque value (path, version, raw error) that must be printed verbatim.
    pub fn raw(value: impl Into<String>) -> Self {
        Self::new(EnvironmentTextCode::Raw, value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentHealthItem {
    /// Stable id: `native_host` | `browser` | `ffmpeg` | `proxy` | `save_dir` | `disk` | `database`.
    pub id: String,
    pub status: EnvironmentHealthStatus,
    pub summary: EnvironmentText,
    /// Detail fragments, joined with a space by the frontend. Empty when the
    /// item has nothing to add beyond its summary.
    pub detail: Vec<EnvironmentText>,
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
    pub message: EnvironmentText,
    /// When set, the frontend should scroll/expand this Settings section.
    pub focus_section: Option<String>,
    /// True when the caller should re-run `get_environment_health`.
    pub refresh: bool,
}
