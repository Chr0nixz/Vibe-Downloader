use serde::{Deserialize, Serialize};
use specta::Type;

use crate::models::Task;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BrowserKind {
    Chrome,
    Edge,
    Firefox,
    Safari,
    Brave,
    Opera,
    Vivaldi,
    Chromium,
}

impl BrowserKind {
    /// Inverse of `as_str` for reading rows back from SQLite; unknown values
    /// (future kinds written by a newer build) are skipped by callers instead
    /// of being surfaced as a broken diagnostics entry.
    pub fn from_db_str(value: &str) -> Option<Self> {
        Some(match value {
            "chrome" => Self::Chrome,
            "edge" => Self::Edge,
            "firefox" => Self::Firefox,
            "safari" => Self::Safari,
            "brave" => Self::Brave,
            "opera" => Self::Opera,
            "vivaldi" => Self::Vivaldi,
            "chromium" => Self::Chromium,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Edge => "edge",
            Self::Firefox => "firefox",
            Self::Safari => "safari",
            Self::Brave => "brave",
            Self::Opera => "opera",
            Self::Vivaldi => "vivaldi",
            Self::Chromium => "chromium",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Chrome => "Google Chrome",
            Self::Edge => "Microsoft Edge",
            Self::Firefox => "Mozilla Firefox",
            Self::Safari => "Safari",
            Self::Brave => "Brave",
            Self::Opera => "Opera",
            Self::Vivaldi => "Vivaldi",
            Self::Chromium => "Chromium",
        }
    }

    pub fn all() -> [Self; 8] {
        [
            Self::Chrome,
            Self::Edge,
            Self::Firefox,
            Self::Safari,
            Self::Brave,
            Self::Opera,
            Self::Vivaldi,
            Self::Chromium,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserIntegrationEntry {
    pub browser: BrowserKind,
    pub display_name: String,
    pub supported_on_platform: bool,
    pub detected: bool,
    /// Best-effort version read from the OS (Windows registry only today);
    /// `None` means "unknown", which the UI renders as such — never a guess.
    pub browser_version: Option<String>,
    pub manifest_installed: bool,
    pub manifest_path: Option<String>,
    pub extension_load_path: Option<String>,
    pub extension_id: Option<String>,
    pub profile: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserIntegrationStatus {
    pub native_host_name: String,
    pub native_host_path: Option<String>,
    pub native_host_ready: bool,
    pub native_host_error: Option<String>,
    pub extension_core_path: Option<String>,
    pub capture_available: bool,
    pub experimental_capture_enabled: bool,
    pub realtime: BrowserRealtimeStatus,
    pub capture: BrowserCaptureSettings,
    pub browsers: Vec<BrowserIntegrationEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserRealtimeStatus {
    pub ws_url: Option<String>,
    pub connected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserCaptureSettings {
    pub experimental_capture_enabled: bool,
    pub auto_intercept: bool,
    pub forward_headers: bool,
    pub forward_headers_mode: BrowserForwardHeadersMode,
    pub min_size_bytes: String,
    pub file_extensions: Vec<String>,
    pub site_rules: Vec<BrowserSiteRule>,
    pub allow_intranet_handoff: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserForwardHeadersMode {
    Ask,
    Enabled,
    Disabled,
}

impl BrowserForwardHeadersMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSiteRule {
    pub id: String,
    pub host_pattern: String,
    pub include_subdomains: bool,
    pub mode: BrowserSiteRuleMode,
    pub min_size_bytes: Option<String>,
    pub file_extensions: Vec<String>,
    pub forward_headers: Option<bool>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserSiteRuleMode {
    Auto,
    Ask,
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserExtensionPackage {
    pub target: String,
    pub package_path: String,
    pub sha256: String,
    pub install_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserExtensionExportResult {
    pub output_dir: String,
    pub install_guide_path: String,
    pub packages: Vec<BrowserExtensionPackage>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserIntegrationUpdateInput {
    pub browsers: Vec<BrowserKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserCaptureSettingsInput {
    pub experimental_capture_enabled: Option<bool>,
    pub auto_intercept: Option<bool>,
    pub forward_headers: Option<bool>,
    pub forward_headers_mode: Option<BrowserForwardHeadersMode>,
    pub min_size_bytes: Option<String>,
    pub file_extensions: Option<Vec<String>>,
    pub site_rules: Option<Vec<BrowserSiteRule>>,
    pub allow_intranet_handoff: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserForwardedHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHandoffInput {
    pub version: i32,
    pub request_id: String,
    pub browser: BrowserKind,
    pub action: String,
    pub url: String,
    pub source: Option<String>,
    pub browser_download_id: Option<String>,
    pub page_url: Option<String>,
    pub referrer: Option<String>,
    pub user_agent: Option<String>,
    pub suggested_file_name: Option<String>,
    pub total_bytes: Option<String>,
    pub mime: Option<String>,
    pub headers_available: Option<bool>,
    pub header_consent_state: Option<String>,
    pub forwarded_headers: Option<Vec<BrowserForwardedHeader>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHandoffResult {
    pub request_id: String,
    pub status: String,
    pub task: Option<Task>,
    pub error_message: Option<String>,
}

/// One row of the `browser_messages` diagnostics table for the integration
/// center history panel. `url` is the SEC-06 query-stripped copy stored at
/// insert time; `status` is `received` or `failed` (duplicates are never
/// persisted, by design).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHandoffRecord {
    pub request_id: String,
    pub browser: BrowserKind,
    pub url: String,
    pub status: String,
    pub error_message: Option<String>,
    pub created_at: String,
}

/// Window + totals for the handoff history panel: `entries` is the recent
/// window (pruned rows are gone), the counts and `last_handoff_at` cover the
/// whole table so "最近 N 条" and "累计" never disagree silently.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHandoffHistory {
    pub entries: Vec<BrowserHandoffRecord>,
    pub received_count: u32,
    pub failed_count: u32,
    pub last_handoff_at: Option<String>,
}

/// Tasks whose browser-supplied auth headers expired before success
/// (`auth_headers_expired` / `auth_headers_unavailable`) and are still in a
/// failure state — the recovery path is FUN-03: re-sending the same URL from
/// the browser refreshes the headers and requeues the task.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExpiredAuthHeaderTask {
    pub task_id: String,
    pub file_name: String,
    pub url: String,
    pub status: String,
    pub error_code: String,
    pub updated_at: Option<String>,
}

/// Result of spawning `vibe-native-host --self-check`. `available: false`
/// means the host binary could not be run or its output could not be trusted;
/// the separate `ok` flag distinguishes "ran and failed" from "never ran".
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserNativeHostSelfCheck {
    pub available: bool,
    pub ok: bool,
    pub version: Option<String>,
    pub protocol_version: Option<u32>,
    pub native_host_path: Option<String>,
    pub app_path: Option<String>,
    pub error_message: Option<String>,
}
