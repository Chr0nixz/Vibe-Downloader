mod backup;
mod browser_messages;
mod classification_rules;
mod connection;
mod dash;
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
pub use self::backup::{
    apply_pending_restore_if_any, current_schema_version, materialize_and_verify_backup_db,
    pack_backup_file, parse_backup_bytes, pending_restore_path, read_backup_file,
    snapshot_database_to_path, write_backup_file, BackupManifest, BACKUP_FORMAT_VERSION,
    CREDENTIALS_POLICY_MACHINE_BOUND,
};
pub use self::browser_messages::{
    browser_message_exists, insert_browser_message, latest_browser_error,
    update_browser_message_status,
};
pub use self::classification_rules::{
    apply_classification_rules, create_classification_rule, delete_classification_rule,
    get_classification_rule, list_classification_rules, match_classification_rule,
    reorder_classification_rules, update_classification_rule,
};
pub use self::connection::{
    begin_immediate, connect, connect_for_startup, reset_database_files, wal_checkpoint,
    wal_file_size_bytes, DatabaseConnectOutcome, DatabaseRecovery, DbConnection,
};
pub use self::dash::{
    bulk_upsert_dash_segments, dash_finish_requested, dash_segment_cursor,
    existing_dash_downloaded_bytes, existing_dash_segment_keys, get_dash_task, list_dash_segments,
    list_dash_segments_page, request_dash_finish, reset_dash_segments_for_task,
    update_dash_segment_status, upsert_dash_segment, upsert_dash_task, DashSegmentRecord,
    DashSegmentUpsert, DashTaskRecord, DashTaskUpsert,
};
pub use self::events::{
    get_latest_pause_event_type, insert_task_event, insert_task_event_in_tx, list_task_events_page,
    prune_task_events, TASK_EVENTS_MAX_AGE_DAYS, TASK_EVENTS_MAX_PER_TASK,
};
pub use self::hash::update_hash_verification;
pub use self::hls::{
    bulk_upsert_hls_segments, get_hls_task, hls_finish_requested, hls_segment_cursor,
    list_hls_segments, list_hls_segments_page, request_hls_finish, reset_hls_segments_for_task,
    update_hls_last_media_sequence, update_hls_segment_status, upsert_hls_segment, upsert_hls_task,
    HlsSegmentRecord, HlsSegmentUpsert, HlsTaskRecord, HlsTaskUpsert,
};
pub use self::metalink::{
    insert_metalink_resource, list_healthy_mirrors_for_file, list_metalink_resources_for_file,
    list_metalink_resources_for_task, mark_metalink_resource_attempted,
    mark_metalink_resource_completed, mark_metalink_resource_failed, mark_mirror_unsupported_range,
    promote_metalink_resource_for_retry, reset_metalink_resource_statuses,
    set_metalink_mirror_cooldown, update_metalink_resource_validators, update_mirror_speed,
    upsert_metalink_task, MetalinkResourceInsert, MetalinkResourceRecord, MetalinkTaskUpsert,
};
pub use self::request_diagnostics::{
    insert_request_diagnostic, list_request_diagnostics_page, prune_request_diagnostics,
    REQUEST_DIAGNOSTICS_MAX_AGE_DAYS, REQUEST_DIAGNOSTICS_MAX_PER_TASK,
};
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
    update_segments_status_for_task, update_segments_status_for_task_in_tx, SegmentSplit,
};
pub use self::settings::{
    clipboard_monitor_enabled, delete_to_trash_enabled, duration_until_next_window_boundary,
    get_bt_upload_limit_bps_setting, get_ffmpeg_path_setting, get_settings,
    local_time_window_active, normalize_accent_color, normalize_ffmpeg_path, normalize_local_time,
    normalize_multi_connection_threshold_bytes, normalize_proxy_mode, normalize_proxy_no_proxy,
    normalize_proxy_optional, normalize_proxy_url, normalize_speed_limit_bps,
    parse_multi_connection_threshold_bytes, parse_speed_limit_bps, upsert_settings,
};
pub use self::sftp::{
    forget_sftp_known_host, list_sftp_known_hosts, verify_or_record_sftp_host_key, SftpKnownHost,
};
pub use self::task_checksums::{
    insert_task_checksum_record, list_task_checksum_records, list_task_checksum_records_for_file,
    list_task_checksum_records_for_tasks, update_task_checksum_record,
};
pub use self::task_credentials::{
    legacy_credentials_from_url, migrate_legacy_ftp_credentials, resolve_task_credentials,
    upsert_task_credentials, TaskCredentials,
};
pub use self::task_files::{
    bump_task_files_version, bump_task_files_version_in_tx, insert_task_file_record,
    insert_task_file_record_in_tx, list_task_file_records, list_task_file_records_for_tasks,
    update_task_file_progress, update_task_file_selection, update_task_file_selection_in_tx,
    update_task_files_progress_batch,
};
pub use self::task_proxy::{
    get_task_proxy_settings, resolve_probe_proxy_config, resolve_task_proxy_config,
    upsert_task_proxy_settings, validate_task_proxy_protocol,
};
pub use self::task_records::{
    find_duplicate_task_record, get_task_record, get_task_record_in_tx, insert_task_record,
    insert_task_record_in_tx, list_browser_realtime_task_records, list_paused_schedulable_tasks,
    list_queued_task_records, list_reserved_final_paths, list_task_ids_by_statuses,
    list_task_records, list_task_records_by_ids, list_task_records_cursor, list_task_records_page,
    next_queue_position, next_retry_after_at, reorder_queued_tasks, task_filter_options,
    task_stats_snapshot, update_task_transfer_options, TaskFilterOptions, TaskListPage,
    TaskListQuery, TaskTransferOptionsUpdate,
};
pub use self::task_state::{
    checkpoint_task_progress, clear_tasks, complete_segment, complete_task, complete_task_segment,
    complete_unknown_size_task, delete_segments_for_task, delete_task_files_for_task,
    delete_task_record, delete_task_records_batch, mark_task_failed_if_active,
    reset_interrupted_tasks, reset_task_download_state, update_task_and_segment_progress,
    update_task_final_path, update_task_health_summary, update_task_progress,
    update_task_retry_after, update_task_retry_after_in_tx, update_task_runtime_progress,
    update_task_save_target, update_task_status, update_task_status_in_tx, TaskProgressCheckpoint,
};
pub use self::torrent::{
    get_torrent_runtime_snapshot, torrent_seed_ratio_limit, torrent_seed_time_limit_seconds,
    torrent_seeding_enabled, torrent_seeding_policy, update_task_remote_metadata,
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
