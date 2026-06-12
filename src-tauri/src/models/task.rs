use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::proxy::AppProxyMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed,
    Retrying,
    WaitingNetwork,
    NeedsAttention,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum HashVerificationStatus {
    NotRequested,
    Pending,
    Verified,
    Failed,
}

impl HashVerificationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotRequested => "not_requested",
            Self::Pending => "pending",
            Self::Verified => "verified",
            Self::Failed => "failed",
        }
    }

    pub fn from_db_str(value: &str) -> Self {
        match value {
            "pending" => Self::Pending,
            "verified" => Self::Verified,
            "failed" => Self::Failed,
            _ => Self::NotRequested,
        }
    }
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Downloading => "downloading",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Retrying => "retrying",
            Self::WaitingNetwork => "waiting_network",
            Self::NeedsAttention => "needs_attention",
        }
    }

    pub fn from_db_str(value: &str) -> Self {
        match value {
            "downloading" => Self::Downloading,
            "paused" => Self::Paused,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "retrying" => Self::Retrying,
            "waiting_network" => Self::WaitingNetwork,
            "needs_attention" => Self::NeedsAttention,
            _ => Self::Queued,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    SingleFile,
    MultiFile,
    Manifest,
}

impl TaskKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SingleFile => "single_file",
            Self::MultiFile => "multi_file",
            Self::Manifest => "manifest",
        }
    }

    pub fn from_db_str(value: &str) -> Self {
        match value {
            "multi_file" => Self::MultiFile,
            "manifest" => Self::Manifest,
            _ => Self::SingleFile,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EngineCapabilities {
    pub supports_resume: bool,
    pub supports_parallel: bool,
    pub supports_multi_file: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub url: String,
    pub final_url: Option<String>,
    pub protocol: String,
    pub task_kind: TaskKind,
    pub file_name: String,
    pub save_dir: String,
    pub temp_path: Option<String>,
    pub final_path: Option<String>,
    pub total_size: String,
    pub downloaded_bytes: String,
    pub status: TaskStatus,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
    pub supports_resume: bool,
    pub supports_parallel: bool,
    pub supports_multi_file: bool,
    pub source_key: String,
    pub connection_count: i32,
    pub speed_bps: String,
    pub health_summary: Option<String>,
    pub error_message: Option<String>,
    pub error_code: Option<String>,
    pub recovery_actions: Vec<RecoveryAction>,
    pub retry_after_at: Option<String>,
    pub failure_category: Option<TaskFailureCategory>,
    pub expected_hash_sha256: Option<String>,
    pub actual_hash_sha256: Option<String>,
    pub hash_status: HashVerificationStatus,
    pub hash_error: Option<String>,
    pub hash_verified_at: Option<String>,
    pub files: Vec<TaskFile>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct TaskRecord {
    pub id: String,
    pub url: String,
    pub final_url: Option<String>,
    pub protocol: String,
    pub task_kind: TaskKind,
    pub file_name: String,
    pub save_dir: String,
    pub temp_path: Option<String>,
    pub final_path: Option<String>,
    pub total_size: i64,
    pub downloaded_bytes: i64,
    pub status: TaskStatus,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
    pub supports_resume: bool,
    pub supports_parallel: bool,
    pub supports_multi_file: bool,
    pub source_key: String,
    pub connection_count: i32,
    pub speed_bps: i64,
    pub health_summary: Option<String>,
    pub error_message: Option<String>,
    pub error_code: Option<String>,
    pub recovery_actions: Vec<RecoveryAction>,
    pub retry_after_at: Option<String>,
    pub expected_hash_sha256: Option<String>,
    pub actual_hash_sha256: Option<String>,
    pub hash_status: HashVerificationStatus,
    pub hash_error: Option<String>,
    pub hash_verified_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskFile {
    pub id: String,
    pub task_id: String,
    pub relative_path: String,
    pub file_name: String,
    pub save_dir: String,
    pub temp_path: Option<String>,
    pub final_path: Option<String>,
    pub total_size: String,
    pub downloaded_bytes: String,
    pub selected: bool,
    pub status: TaskStatus,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskFileRecord {
    pub id: String,
    pub task_id: String,
    pub relative_path: String,
    pub file_name: String,
    pub save_dir: String,
    pub temp_path: Option<String>,
    pub final_path: Option<String>,
    pub total_size: i64,
    pub downloaded_bytes: i64,
    pub selected: bool,
    pub status: TaskStatus,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskSegment {
    pub id: String,
    pub task_id: String,
    pub file_id: Option<String>,
    pub unit_kind: String,
    pub range_start: String,
    pub range_end: String,
    pub downloaded_until: String,
    pub speed_bps: String,
    pub status: SegmentStatus,
    pub retry_count: i32,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskSegmentRecord {
    pub id: String,
    pub task_id: String,
    pub file_id: Option<String>,
    pub unit_kind: String,
    pub range_start: i64,
    pub range_end: i64,
    pub downloaded_until: i64,
    pub speed_bps: i64,
    pub status: SegmentStatus,
    pub retry_count: i32,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvent {
    pub id: String,
    pub task_id: String,
    pub event_type: String,
    pub payload: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RequestDiagnostic {
    pub id: String,
    pub task_id: String,
    pub method: String,
    pub url: String,
    pub range_header: Option<String>,
    pub status_code: Option<i32>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_length: Option<String>,
    pub error_message: Option<String>,
    pub retry_count: i32,
    pub duration_ms: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct RequestDiagnosticRecord {
    pub task_id: String,
    pub method: String,
    pub url: String,
    pub range_header: Option<String>,
    pub status_code: Option<i32>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_length: Option<i64>,
    pub error_message: Option<String>,
    pub retry_count: i32,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SegmentSummary {
    pub total: i32,
    pub active: i32,
    pub completed: i32,
    pub failed: i32,
    pub downloaded_bytes: String,
    pub speed_bps: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HashVerificationState {
    pub task_id: String,
    pub expected_sha256: Option<String>,
    pub actual_sha256: Option<String>,
    pub status: HashVerificationStatus,
    pub error_message: Option<String>,
    pub verified_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BatchImportItem {
    pub input_url: String,
    pub normalized_url: Option<String>,
    pub duplicate: bool,
    pub valid: bool,
    pub file_name: Option<String>,
    pub total_size: Option<String>,
    pub content_type: Option<String>,
    pub supports_resume: bool,
    pub error_message: Option<String>,
    pub task: Option<Task>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BatchImportResult {
    pub items: Vec<BatchImportItem>,
    pub created_count: i32,
    pub failed_count: i32,
    pub duplicate_count: i32,
}

impl From<TaskSegmentRecord> for TaskSegment {
    fn from(record: TaskSegmentRecord) -> Self {
        Self {
            id: record.id,
            task_id: record.task_id,
            file_id: record.file_id,
            unit_kind: record.unit_kind,
            range_start: record.range_start.to_string(),
            range_end: record.range_end.to_string(),
            downloaded_until: record.downloaded_until.to_string(),
            speed_bps: record.speed_bps.to_string(),
            status: record.status,
            retry_count: record.retry_count,
            last_error: record.last_error,
        }
    }
}

impl From<TaskFileRecord> for TaskFile {
    fn from(record: TaskFileRecord) -> Self {
        Self {
            id: record.id,
            task_id: record.task_id,
            relative_path: record.relative_path,
            file_name: record.file_name,
            save_dir: record.save_dir,
            temp_path: record.temp_path,
            final_path: record.final_path,
            total_size: record.total_size.to_string(),
            downloaded_bytes: record.downloaded_bytes.to_string(),
            selected: record.selected,
            status: record.status,
            content_type: record.content_type,
        }
    }
}

impl From<TaskRecord> for Task {
    fn from(record: TaskRecord) -> Self {
        let failure_category = failure_category_for_code(record.error_code.as_deref());
        Self {
            id: record.id,
            url: record.url,
            final_url: record.final_url,
            protocol: record.protocol,
            task_kind: record.task_kind,
            file_name: record.file_name,
            save_dir: record.save_dir,
            temp_path: record.temp_path,
            final_path: record.final_path,
            total_size: record.total_size.to_string(),
            downloaded_bytes: record.downloaded_bytes.to_string(),
            status: record.status,
            etag: record.etag,
            last_modified: record.last_modified,
            content_type: record.content_type,
            supports_resume: record.supports_resume,
            supports_parallel: record.supports_parallel,
            supports_multi_file: record.supports_multi_file,
            source_key: record.source_key,
            connection_count: record.connection_count,
            speed_bps: record.speed_bps.to_string(),
            health_summary: record.health_summary,
            error_message: record.error_message,
            error_code: record.error_code,
            recovery_actions: record.recovery_actions,
            retry_after_at: record.retry_after_at,
            failure_category,
            expected_hash_sha256: record.expected_hash_sha256,
            actual_hash_sha256: record.actual_hash_sha256,
            hash_status: record.hash_status,
            hash_error: record.hash_error,
            hash_verified_at: record.hash_verified_at,
            files: Vec::new(),
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SegmentStatus {
    Pending,
    Downloading,
    Completed,
    Failed,
}

impl SegmentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Downloading => "downloading",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub fn from_db_str(value: &str) -> Self {
        match value {
            "downloading" => Self::Downloading,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskProgressPayload {
    pub task_id: String,
    pub downloaded_bytes: String,
    pub total_size: String,
    pub speed_bps: String,
    pub connection_count: i32,
    pub status: TaskStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskUpdatedPayload {
    pub task: Task,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProbeTaskPayload {
    pub final_url: String,
    pub file_name: String,
    pub protocol: String,
    pub task_kind: TaskKind,
    pub capabilities: EngineCapabilities,
    pub files: Vec<ProbedFile>,
    pub total_size: String,
    pub source_key: String,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProbedFile {
    pub relative_path: String,
    pub size: String,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppFontFamily {
    System,
    SourceHanSansSc,
}

impl AppFontFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::SourceHanSansSc => "source_han_sans_sc",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppAccentColor {
    Blue,
    Purple,
    Teal,
    Green,
    Orange,
    Rose,
    Indigo,
    Amber,
}

impl AppAccentColor {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Blue => "blue",
            Self::Purple => "purple",
            Self::Teal => "teal",
            Self::Green => "green",
            Self::Orange => "orange",
            Self::Rose => "rose",
            Self::Indigo => "indigo",
            Self::Amber => "amber",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub max_active_tasks: i32,
    pub default_save_dir: String,
    pub global_speed_limit_bps: Option<String>,
    pub multi_connection_threshold_bytes: String,
    pub segment_count: i32,
    pub max_connections_per_host: i32,
    pub system_notifications: bool,
    pub close_to_tray: bool,
    pub start_on_boot: bool,
    pub floating_window_enabled: bool,
    pub clipboard_monitor_enabled: bool,
    pub font_family: AppFontFamily,
    pub accent_color: AppAccentColor,
    pub proxy_mode: AppProxyMode,
    pub proxy_url: String,
    pub proxy_no_proxy: String,
    pub proxy_username: String,
    pub proxy_password_saved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppErrorPayload {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    Retry,
    RetryLater,
    ChooseAnotherName,
    ChooseAnotherFolder,
    Restart,
    OpenFolder,
    CheckUrl,
    FreeDiskSpace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TaskFailureCategory {
    RemoteChanged,
    ResumeUnavailable,
    TempFile,
    DiskWrite,
    Http,
    Auth,
    Other,
}

pub fn failure_category_for_code(code: Option<&str>) -> Option<TaskFailureCategory> {
    match code? {
        "remote_changed" => Some(TaskFailureCategory::RemoteChanged),
        "resume_unavailable" => Some(TaskFailureCategory::ResumeUnavailable),
        "temp_file_missing" | "temp_file_smaller_than_progress" => {
            Some(TaskFailureCategory::TempFile)
        }
        "disk_write_failed" => Some(TaskFailureCategory::DiskWrite),
        "auth_headers_expired" | "auth_headers_unavailable" => Some(TaskFailureCategory::Auth),
        value if value.starts_with("http_") || value == "server_rate_limited" => {
            Some(TaskFailureCategory::Http)
        }
        _ => Some(TaskFailureCategory::Other),
    }
}

impl RecoveryAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Retry => "retry",
            Self::RetryLater => "retry_later",
            Self::ChooseAnotherName => "choose_another_name",
            Self::ChooseAnotherFolder => "choose_another_folder",
            Self::Restart => "restart",
            Self::OpenFolder => "open_folder",
            Self::CheckUrl => "check_url",
            Self::FreeDiskSpace => "free_disk_space",
        }
    }
}

impl std::str::FromStr for RecoveryAction {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "retry" => Ok(Self::Retry),
            "retry_later" => Ok(Self::RetryLater),
            "choose_another_name" => Ok(Self::ChooseAnotherName),
            "choose_another_folder" => Ok(Self::ChooseAnotherFolder),
            "restart" => Ok(Self::Restart),
            "open_folder" => Ok(Self::OpenFolder),
            "check_url" => Ok(Self::CheckUrl),
            "free_disk_space" => Ok(Self::FreeDiskSpace),
            _ => Err(()),
        }
    }
}

impl AppErrorPayload {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        recoverable: bool,
        actions: Vec<&str>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            recoverable,
            actions: actions.into_iter().map(str::to_string).collect(),
        }
    }

    pub fn command_error(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| self.message.clone())
    }

    pub fn final_path_conflict(path: &str) -> Self {
        Self::new(
            "final_path_conflict",
            format!(
                "The destination file already exists and no alternate name is available: {path}"
            ),
            true,
            vec!["choose_another_name", "choose_another_folder", "retry"],
        )
    }

    pub fn disk_write_failed(message: impl Into<String>) -> Self {
        Self::new(
            "disk_write_failed",
            message,
            true,
            vec!["free_disk_space", "choose_another_folder", "retry"],
        )
    }

    pub fn http_status(code: &str, message: impl Into<String>, recoverable: bool) -> Self {
        let actions = if recoverable {
            vec!["retry_later"]
        } else {
            vec!["check_url", "retry"]
        };
        Self::new(code, message, recoverable, actions)
    }

    pub fn auth_headers_expired() -> Self {
        Self::new(
            "auth_headers_expired",
            "Browser authentication headers expired. Send this download from the browser again or restart it.",
            true,
            vec!["check_url", "restart"],
        )
    }

    pub fn auth_headers_unavailable(message: impl Into<String>) -> Self {
        Self::new(
            "auth_headers_unavailable",
            message,
            true,
            vec!["check_url", "restart"],
        )
    }
}

pub fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

#[allow(dead_code)]
pub fn parse_iso(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
