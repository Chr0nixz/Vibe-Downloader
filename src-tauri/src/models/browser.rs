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
    pub extension_core_path: Option<String>,
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
