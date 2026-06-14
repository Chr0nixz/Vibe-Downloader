pub mod browser;
pub mod task;

pub use browser::{
    BrowserCaptureSettings, BrowserCaptureSettingsInput, BrowserExtensionExportResult,
    BrowserExtensionPackage, BrowserForwardHeadersMode, BrowserForwardedHeader,
    BrowserHandoffInput, BrowserHandoffResult, BrowserIntegrationEntry, BrowserIntegrationStatus,
    BrowserIntegrationUpdateInput, BrowserKind, BrowserRealtimeStatus, BrowserSiteRule,
    BrowserSiteRuleMode,
};
pub use task::{
    AppAccentColor, AppErrorPayload, AppFontFamily, AppSettings, BatchImportItem,
    BatchImportResult, ChecksumAlgorithm, CompletionAction, CompletionActionRequestedPayload, EngineCapabilities,
    FtpDirectoryEntry, FtpDirectoryProbe, HashVerificationState, HashVerificationStatus,
    HlsVariant, ProbeTaskPayload, ProbedFile, RecoveryAction, RequestDiagnostic,
    RequestDiagnosticRecord, SegmentStatus, SegmentSummary, Task, TaskChecksum, TaskChecksumRecord, TaskEvent, TaskFailureCategory, TaskFile,
    TaskFileRecord, TaskKind, TaskPriority, TaskProgressPayload, TaskProxyMode,
    TaskProxySettings, TaskProxySettingsInput, TaskProxySettingsRecord, TaskRecord, TaskSegment,
    TaskSegmentRecord, TaskStatus, TaskUpdatedPayload, TorrentRuntimeSnapshot,
    TorrentRuntimeSnapshotRecord, TorrentTrackerStatus,
};
