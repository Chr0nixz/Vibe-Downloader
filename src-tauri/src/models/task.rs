use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub url: String,
    pub final_url: Option<String>,
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
    pub supports_range: bool,
    pub source_host: String,
    pub connection_count: i32,
    pub speed_bps: String,
    pub health_summary: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct TaskRecord {
    pub id: String,
    pub url: String,
    pub final_url: Option<String>,
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
    pub supports_range: bool,
    pub source_host: String,
    pub connection_count: i32,
    pub speed_bps: i64,
    pub health_summary: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<TaskRecord> for Task {
    fn from(record: TaskRecord) -> Self {
        Self {
            id: record.id,
            url: record.url,
            final_url: record.final_url,
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
            supports_range: record.supports_range,
            source_host: record.source_host,
            connection_count: record.connection_count,
            speed_bps: record.speed_bps.to_string(),
            health_summary: record.health_summary,
            error_message: record.error_message,
            created_at: record.created_at,
            updated_at: record.updated_at,
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
pub struct ProbeTaskPayload {
    pub final_url: String,
    pub file_name: String,
    pub total_size: String,
    pub supports_range: bool,
    pub source_host: String,
    pub content_type: Option<String>,
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
