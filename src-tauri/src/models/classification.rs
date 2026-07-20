use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ClassificationMatchKind {
    /// Match by file extension, e.g., "mp4"
    Extension,
    /// Match by MIME prefix, e.g., "video/"
    Mime,
    /// Match by URL keyword containment, e.g., "example.com/video"
    UrlContains,
}

impl ClassificationMatchKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Extension => "extension",
            Self::Mime => "mime",
            Self::UrlContains => "url_contains",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "extension" => Some(Self::Extension),
            "mime" => Some(Self::Mime),
            "url_contains" => Some(Self::UrlContains),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationRule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub position: i32,
    pub match_kind: ClassificationMatchKind,
    pub pattern: String,
    pub target_subdir: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationRuleInput {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub position: Option<i32>,
    pub match_kind: Option<ClassificationMatchKind>,
    pub pattern: Option<String>,
    pub target_subdir: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewClassificationInput {
    pub url: String,
    pub file_name: Option<String>,
    pub content_type: Option<String>,
    /// When set, preview reports a manual override and skips automatic rules.
    pub category_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewClassificationInputsUsed {
    pub url: String,
    pub file_name: String,
    pub content_type: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewClassificationResult {
    pub matched: bool,
    pub manual_override: bool,
    pub target_subdir: Option<String>,
    pub matched_rule: Option<ClassificationRule>,
    pub effective_save_dir: Option<String>,
    pub inputs_used: PreviewClassificationInputsUsed,
}
