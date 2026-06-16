mod browser_messages;
mod connection;
mod events;
mod hash;
mod hls;
mod metalink;
mod request_diagnostics;
mod request_headers;
mod segment_planner;
mod segments;
mod settings;
mod sftp;
mod task_checksums;
mod task_credentials;
mod task_files;
mod task_proxy;
mod task_records;
mod task_state;
mod torrent;
pub use self::browser_messages::{
    browser_message_exists, insert_browser_message, latest_browser_error,
    update_browser_message_status,
};
pub use self::connection::{connect, DbConnection};
pub use self::events::{insert_task_event, list_task_events_page};
pub use self::hash::update_hash_verification;
pub use self::hls::{
    get_hls_task, hls_finish_requested, list_hls_segments, request_hls_finish,
    reset_hls_segments_for_task, update_hls_last_media_sequence, update_hls_segment_status,
    upsert_hls_segment, upsert_hls_task, HlsSegmentRecord, HlsSegmentUpsert, HlsTaskRecord,
    HlsTaskUpsert,
};
pub use self::metalink::{
    insert_metalink_resource, list_metalink_resources_for_file, mark_metalink_resource_completed,
    mark_metalink_resource_failed, upsert_metalink_task, MetalinkResourceInsert,
    MetalinkResourceRecord, MetalinkTaskUpsert,
};
pub use self::request_diagnostics::{insert_request_diagnostic, list_request_diagnostics_page};
pub use self::request_headers::{
    clear_all_task_request_headers, clear_expired_task_request_headers,
    delete_task_request_headers, resolve_task_request_headers, upsert_task_request_headers,
    TASK_REQUEST_HEADERS_TTL_HOURS,
};
pub use self::segment_planner::{
    planned_segment_count, planned_segment_count_with_plan, planned_segments_for_task,
    planned_segments_for_task_with_plan,
};
pub use self::segments::{
    ensure_single_segment_for_task, ensure_task_segments, ensure_task_segments_with_settings,
    get_first_segment_record, insert_segment_record, list_segment_records,
    list_segment_records_cursor, list_segment_records_paged, segment_downloaded_bytes,
    segment_summary, split_largest_remaining_segment, total_segment_downloaded_bytes,
    update_segment_downloaded_until, update_segment_progress, update_segment_range_end,
    update_segment_retry, update_segment_runtime_progress, update_segment_status,
    update_segments_status_for_task, SegmentSplit,
};
pub use self::settings::{
    clipboard_monitor_enabled, get_settings, local_time_window_active, normalize_accent_color,
    normalize_font_family, normalize_local_time, normalize_multi_connection_threshold_bytes,
    normalize_proxy_mode, normalize_proxy_no_proxy, normalize_proxy_optional, normalize_proxy_url,
    normalize_speed_limit_bps, parse_multi_connection_threshold_bytes, parse_speed_limit_bps,
    upsert_settings,
};
pub use self::sftp::verify_or_record_sftp_host_key;
pub use self::task_checksums::{
    insert_task_checksum_record, list_task_checksum_records, list_task_checksum_records_for_file,
    list_task_checksum_records_for_tasks, update_task_checksum_record,
};
pub use self::task_credentials::{
    legacy_credentials_from_url, migrate_legacy_ftp_credentials, resolve_task_credentials,
    upsert_task_credentials, TaskCredentials,
};
pub use self::task_files::{
    insert_task_file_record, list_task_file_records, list_task_file_records_for_tasks,
    update_task_file_progress, update_task_file_selection,
};
pub use self::task_proxy::{
    get_task_proxy_settings, resolve_task_proxy_config, upsert_task_proxy_settings,
    validate_task_proxy_protocol,
};
pub use self::task_records::{
    find_duplicate_task_record, get_task_record, insert_task_record,
    list_browser_realtime_task_records, list_queued_task_records, list_task_records,
    list_task_records_cursor, list_task_records_page, next_queue_position, next_retry_after_at,
    task_filter_options, task_stats_snapshot, update_task_transfer_options, TaskFilterOptions,
    TaskListPage, TaskListQuery, TaskTransferOptionsUpdate,
};
pub use self::task_state::{
    clear_tasks, complete_segment, complete_task, complete_task_segment,
    complete_unknown_size_task, delete_segments_for_task, delete_task_files_for_task,
    delete_task_record, reset_interrupted_tasks, reset_task_download_state,
    update_task_and_segment_progress, update_task_final_path, update_task_health_summary,
    update_task_progress, update_task_retry_after, update_task_runtime_progress,
    update_task_save_target, update_task_status,
};
pub use self::torrent::{
    get_torrent_runtime_snapshot, torrent_seeding_enabled, update_task_remote_metadata,
    update_task_torrent_metadata, update_torrent_seeding, upsert_torrent_runtime_snapshot,
    upsert_torrent_task, TaskRemoteMetadataUpdate, TaskTorrentMetadataUpdate,
    TorrentRuntimeSnapshotUpsert, TorrentTaskUpsert,
};

pub const DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES: i64 = 16 * 1024 * 1024;
pub const MIN_MULTI_CONNECTION_THRESHOLD_BYTES: i64 = 0;
pub const MAX_MULTI_CONNECTION_THRESHOLD_BYTES: i64 = 1024_i64 * 1024 * 1024 * 1024;
pub const DEFAULT_SEGMENT_COUNT: i32 = 4;
pub const MIN_SEGMENT_COUNT: i32 = 1;
pub const MAX_SEGMENT_COUNT: i32 = 8;
pub const MAX_AUTO_SEGMENT_COUNT: usize = 8;
pub const DEFAULT_MAX_CONNECTIONS_PER_HOST: i32 = 8;
pub const MIN_MAX_CONNECTIONS_PER_HOST: i32 = 1;
pub const MAX_MAX_CONNECTIONS_PER_HOST: i32 = 16;
pub const DEFAULT_MAX_ACTIVE_TASKS: i32 = 2;
pub const MIN_MAX_ACTIVE_TASKS: i32 = 1;
pub const MAX_MAX_ACTIVE_TASKS: i32 = 8;
pub const DEFAULT_TASK_PAGE_SIZE: i64 = 100;
pub const MAX_TASK_PAGE_SIZE: i64 = 500;
