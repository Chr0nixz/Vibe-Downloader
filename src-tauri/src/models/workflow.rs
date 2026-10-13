//! Non-sensitive, versioned configuration shared by daily workflows.
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{TaskPriority, TaskProxyMode, TaskRequestProfileInput};

#[derive(Clone, Deserialize, Serialize, Type)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum WorkflowData {
    TaskPreset(TaskPreset),
    SiteProfile(SiteProfile),
    SavedView(SavedView),
}

#[derive(Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskPreset {
    pub save_dir: String,
    pub category_key: Option<String>,
    pub priority: TaskPriority,
    pub task_speed_limit_bps: Option<String>,
    pub obey_schedule: bool,
    pub proxy_mode: TaskProxyMode,
    pub proxy_url: Option<String>,
    pub proxy_no_proxy: Option<String>,
}

#[derive(Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SiteProfile {
    pub origin: String,
    pub profile: TaskRequestProfileInput,
}

#[derive(Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedView {
    pub nav: String,
    pub search: String,
    pub sort_key: String,
    pub sort_direction: String,
    pub file_type: String,
    pub source: String,
    pub failure: String,
    pub resume: String,
}

#[derive(Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowEntry {
    pub id: String,
    pub name: String,
    pub data: WorkflowData,
}

#[derive(Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowLibrary {
    pub entries: Vec<WorkflowEntry>,
    pub unsupported_count: u32,
}

#[derive(Clone, Default, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BulkTaskOptionsPatch {
    pub priority: Option<TaskPriority>,
    pub task_speed_limit_bps: Option<String>,
    pub clear_speed_limit: bool,
    pub category_key: Option<String>,
    pub clear_category: bool,
    pub obey_schedule: Option<bool>,
}

#[derive(Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BulkTaskOptionsInput {
    pub ids: Vec<String>,
    pub patch: BulkTaskOptionsPatch,
}

#[derive(Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BulkTaskOptionsResult {
    pub id: String,
    pub status: String,
    pub error: Option<String>,
}
