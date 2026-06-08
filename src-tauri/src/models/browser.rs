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
    pub browsers: Vec<BrowserIntegrationEntry>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserIntegrationUpdateInput {
    pub browsers: Vec<BrowserKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHandoffInput {
    pub version: i32,
    pub request_id: String,
    pub browser: BrowserKind,
    pub action: String,
    pub url: String,
    pub page_url: Option<String>,
    pub referrer: Option<String>,
    pub user_agent: Option<String>,
    pub suggested_file_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHandoffResult {
    pub request_id: String,
    pub status: String,
    pub task: Option<Task>,
    pub error_message: Option<String>,
}
