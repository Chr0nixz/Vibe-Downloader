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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    Low,
    Normal,
    High,
}

impl TaskPriority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
        }
    }

    pub fn from_db_str(value: &str) -> Self {
        match value {
            "low" => Self::Low,
            "high" => Self::High,
            _ => Self::Normal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ChecksumAlgorithm {
    Sha256,
    Sha512,
    Sha1,
    Md5,
}

impl ChecksumAlgorithm {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
            Self::Sha512 => "sha512",
            Self::Sha1 => "sha1",
            Self::Md5 => "md5",
        }
    }

    pub fn from_db_str(value: &str) -> Self {
        match value {
            "sha512" => Self::Sha512,
            "sha1" => Self::Sha1,
            "md5" => Self::Md5,
            _ => Self::Sha256,
        }
    }

    pub fn is_weak(self) -> bool {
        matches!(self, Self::Sha1 | Self::Md5)
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
    pub task_speed_limit_bps: Option<String>,
    pub priority: TaskPriority,
    pub queue_position: String,
    pub category_key: Option<String>,
    pub obey_schedule: bool,
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
    pub checksums: Vec<TaskChecksum>,
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
    pub task_speed_limit_bps: Option<String>,
    pub priority: TaskPriority,
    pub queue_position: i64,
    pub category_key: Option<String>,
    pub obey_schedule: bool,
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
pub struct TaskChecksum {
    pub id: String,
    pub task_id: String,
    pub file_id: Option<String>,
    pub algorithm: ChecksumAlgorithm,
    pub expected_hash: String,
    pub actual_hash: Option<String>,
    pub status: HashVerificationStatus,
    pub source_kind: String,
    pub source_url: Option<String>,
    pub source_label: Option<String>,
    pub is_primary: bool,
    pub weak: bool,
    pub error_message: Option<String>,
    pub discovered_at: Option<String>,
    pub verified_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct TaskChecksumRecord {
    pub id: String,
    pub task_id: String,
    pub file_id: Option<String>,
    pub algorithm: ChecksumAlgorithm,
    pub expected_hash: String,
    pub actual_hash: Option<String>,
    pub status: HashVerificationStatus,
    pub source_kind: String,
    pub source_url: Option<String>,
    pub source_label: Option<String>,
    pub is_primary: bool,
    pub weak: bool,
    pub error_message: Option<String>,
    pub discovered_at: Option<String>,
    pub verified_at: Option<String>,
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
    pub if_range_header: Option<String>,
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
    pub if_range_header: Option<String>,
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

impl From<TaskChecksumRecord> for TaskChecksum {
    fn from(record: TaskChecksumRecord) -> Self {
        Self {
            id: record.id,
            task_id: record.task_id,
            file_id: record.file_id,
            algorithm: record.algorithm,
            expected_hash: record.expected_hash,
            actual_hash: record.actual_hash,
            status: record.status,
            source_kind: record.source_kind,
            source_url: record.source_url,
            source_label: record.source_label,
            is_primary: record.is_primary,
            weak: record.weak,
            error_message: record.error_message,
            discovered_at: record.discovered_at,
            verified_at: record.verified_at,
            created_at: record.created_at,
            updated_at: record.updated_at,
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
            task_speed_limit_bps: record.task_speed_limit_bps,
            priority: record.priority,
            queue_position: record.queue_position.to_string(),
            category_key: record.category_key,
            obey_schedule: record.obey_schedule,
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
            checksums: Vec::new(),
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
    pub input_url: String,
    pub final_url: String,
    pub file_name: String,
    pub protocol: String,
    pub task_kind: TaskKind,
    pub capabilities: EngineCapabilities,
    pub files: Vec<ProbedFile>,
    pub total_size: String,
    pub source_key: String,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub hls_variants: Vec<HlsVariant>,
    pub probed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HlsVariant {
    pub uri: String,
    pub bandwidth: String,
    pub resolution: Option<String>,
    pub codecs: Option<String>,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProbedFile {
    pub relative_path: String,
    pub size: String,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MetalinkProbeData {
    pub manifest_url: String,
    pub manifest_format: String,
    pub files: Vec<MetalinkFile>,
}

#[derive(Debug, Clone)]
pub struct MetalinkFile {
    pub relative_path: String,
    pub size: i64,
    pub checksums: Vec<MetalinkChecksum>,
    pub resources: Vec<MetalinkResource>,
}

#[derive(Debug, Clone)]
pub struct MetalinkChecksum {
    pub algorithm: ChecksumAlgorithm,
    pub value: String,
    pub weak: bool,
}

#[derive(Debug, Clone)]
pub struct MetalinkResource {
    pub url: String,
    pub priority: i64,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FtpDirectoryEntry {
    pub name: String,
    pub raw: String,
    pub probable_file_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FtpDirectoryProbe {
    pub input_url: String,
    pub directory_url: String,
    pub current_directory: Option<String>,
    pub entries: Vec<FtpDirectoryEntry>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentRuntimeSnapshot {
    pub task_id: String,
    pub metadata_status: String,
    pub completed_pieces: String,
    pub verified_pieces: String,
    pub piece_count: String,
    pub piece_bitfield_base64: Option<String>,
    pub peer_count: String,
    pub seed_count: String,
    pub dht_status: Option<String>,
    pub trackers: Vec<TorrentTrackerStatus>,
    pub upload_bytes: String,
    pub upload_speed_bps: String,
    pub ratio: f64,
    pub seeding_enabled: bool,
    pub seeding_state: String,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentTrackerStatus {
    pub url: String,
    pub status: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TorrentRuntimeSnapshotRecord {
    pub task_id: String,
    pub metadata_status: String,
    pub completed_pieces: i64,
    pub verified_pieces: i64,
    pub piece_count: i64,
    pub piece_bitfield_base64: Option<String>,
    pub peer_count: i64,
    pub seed_count: i64,
    pub dht_status: Option<String>,
    pub trackers_json: Option<String>,
    pub upload_bytes: i64,
    pub upload_speed_bps: i64,
    pub ratio: f64,
    pub seeding_enabled: bool,
    pub seeding_state: String,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
    pub updated_at: String,
}

impl From<TorrentRuntimeSnapshotRecord> for TorrentRuntimeSnapshot {
    fn from(record: TorrentRuntimeSnapshotRecord) -> Self {
        Self {
            task_id: record.task_id,
            metadata_status: record.metadata_status,
            completed_pieces: record.completed_pieces.to_string(),
            verified_pieces: record.verified_pieces.to_string(),
            piece_count: record.piece_count.to_string(),
            piece_bitfield_base64: record.piece_bitfield_base64,
            peer_count: record.peer_count.to_string(),
            seed_count: record.seed_count.to_string(),
            dht_status: record.dht_status,
            trackers: record
                .trackers_json
                .as_deref()
                .and_then(|value| serde_json::from_str(value).ok())
                .unwrap_or_default(),
            upload_bytes: record.upload_bytes.to_string(),
            upload_speed_bps: record.upload_speed_bps.to_string(),
            ratio: record.ratio,
            seeding_enabled: record.seeding_enabled,
            seeding_state: record.seeding_state,
            last_error_code: record.last_error_code,
            last_error_message: record.last_error_message,
            updated_at: record.updated_at,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TaskProxyMode {
    Inherit,
    Off,
    Custom,
}

impl TaskProxyMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inherit => "inherit",
            Self::Off => "off",
            Self::Custom => "custom",
        }
    }

    pub fn from_db_str(value: &str) -> Self {
        match value {
            "off" => Self::Off,
            "custom" => Self::Custom,
            _ => Self::Inherit,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskProxySettings {
    pub task_id: String,
    pub mode: TaskProxyMode,
    pub proxy_url: String,
    pub proxy_username: String,
    pub proxy_password_saved: bool,
    pub no_proxy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskProxySettingsInput {
    pub task_id: String,
    pub mode: TaskProxyMode,
    pub proxy_url: Option<String>,
    pub proxy_username: Option<String>,
    pub proxy_password: Option<String>,
    pub clear_proxy_password: Option<bool>,
    pub no_proxy: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskProxySettingsRecord {
    pub task_id: String,
    pub mode: TaskProxyMode,
    pub proxy_url: Option<String>,
    pub proxy_username: Option<String>,
    pub proxy_password_ciphertext: Option<String>,
    pub nonce: Option<String>,
    pub no_proxy: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CompletionAction {
    None,
    ExitApp,
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CompletionActionRequestedPayload {
    pub action: CompletionAction,
    pub countdown_seconds: i32,
}

impl CompletionAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ExitApp => "exit_app",
            Self::Shutdown => "shutdown",
        }
    }

    pub fn from_db_str(value: &str) -> Self {
        match value {
            "exit_app" => Self::ExitApp,
            "shutdown" => Self::Shutdown,
            _ => Self::None,
        }
    }
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
    pub auto_resume_on_startup: bool,
    pub floating_window_enabled: bool,
    pub clipboard_monitor_enabled: bool,
    pub font_family: AppFontFamily,
    pub accent_color: AppAccentColor,
    pub proxy_mode: AppProxyMode,
    pub proxy_url: String,
    pub proxy_no_proxy: String,
    pub proxy_username: String,
    pub proxy_password_saved: bool,
    pub schedule_download_window_enabled: bool,
    pub schedule_download_window_start: String,
    pub schedule_download_window_end: String,
    pub schedule_speed_limit_window_enabled: bool,
    pub schedule_speed_limit_window_start: String,
    pub schedule_speed_limit_window_end: String,
    pub schedule_speed_limit_bps: Option<String>,
    pub completion_action: CompletionAction,
    pub completion_countdown_seconds: i32,
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
    Hls,
    Metalink,
    Bt,
    Ftp,
    Proxy,
    Schedule,
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
        value if value.starts_with("hls_") => Some(TaskFailureCategory::Hls),
        value if value.starts_with("metalink_") => Some(TaskFailureCategory::Metalink),
        value if value.starts_with("bt_") => Some(TaskFailureCategory::Bt),
        value if value.starts_with("ftp_") => Some(TaskFailureCategory::Ftp),
        value if value.starts_with("proxy_") => Some(TaskFailureCategory::Proxy),
        value if value.starts_with("schedule_") => Some(TaskFailureCategory::Schedule),
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

    pub fn duplicate_task(file_name: &str) -> Self {
        Self::new(
            "duplicate_task",
            format!("A task for this download already exists: {file_name}"),
            true,
            Vec::new(),
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
