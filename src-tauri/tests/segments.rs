use std::time::{SystemTime, UNIX_EPOCH};

use tauri_app_lib::{
    commands::tasks::{local_resume_error, resume_mismatch_message, segment_resume_error},
    db,
    download::ProbeResult,
    models::{
        AppAccentColor, AppSettings, BrowserKind, CompletionAction, HashVerificationStatus,
        SegmentStatus, Task, TaskKind, TaskPriority, TaskRecord, TaskStatus, TaskUpdatedPayload,
    },
};

#[tokio::test]
async fn ensure_single_segment_creates_task_range() {
    let pool = test_pool("create-segment").await;
    let task = sample_task("task-create", 100);
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");

    let segment = db::ensure_single_segment_for_task(&pool, &task)
        .await
        .expect("segment");

    assert_eq!(segment.task_id, task.id);
    assert_eq!(segment.range_start, 0);
    assert_eq!(segment.range_end, 99);
    assert_eq!(segment.downloaded_until, 0);
    assert_eq!(segment.status, SegmentStatus::Pending);
}

#[tokio::test]
async fn small_or_no_range_tasks_keep_one_segment() {
    let pool = test_pool("single-planning").await;
    let small = sample_task(
        "task-small",
        db::DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES - 1,
    );
    db::insert_task_record(&pool, &small)
        .await
        .expect("insert small");
    let small_segments = db::ensure_task_segments(&pool, &small)
        .await
        .expect("small segments");
    assert_eq!(small_segments.len(), 1);

    let mut no_range = sample_task(
        "task-no-range",
        db::DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES,
    );
    no_range.supports_parallel = false;
    db::insert_task_record(&pool, &no_range)
        .await
        .expect("insert no range");
    let no_range_segments = db::ensure_task_segments(&pool, &no_range)
        .await
        .expect("no range segments");
    assert_eq!(no_range_segments.len(), 1);
}

#[tokio::test]
async fn large_range_task_generates_four_non_overlapping_segments() {
    let pool = test_pool("multi-planning").await;
    let total_size = db::DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES + 7;
    let task = sample_task("task-large", total_size);
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");

    let segments = db::ensure_task_segments(&pool, &task)
        .await
        .expect("segments");

    assert_eq!(segments.len(), 4);
    assert_eq!(segments[0].range_start, 0);
    assert_eq!(segments[3].range_end, total_size - 1);

    for window in segments.windows(2) {
        assert_eq!(window[0].range_end + 1, window[1].range_start);
    }

    let covered_bytes = segments
        .iter()
        .map(|segment| segment.range_end - segment.range_start + 1)
        .sum::<i64>();
    assert_eq!(covered_bytes, total_size);
    assert!(segments
        .iter()
        .all(|segment| segment.downloaded_until == segment.range_start));
}

#[tokio::test]
async fn configurable_threshold_and_segment_count_plan_new_segments() {
    let pool = test_pool("configurable-planning").await;
    let task = sample_task("task-configurable", 4 * 1024 * 1024);
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");
    let settings = AppSettings {
        max_active_tasks: 2,
        default_save_dir: "C:\\Downloads".to_string(),
        global_speed_limit_bps: None,
        multi_connection_threshold_bytes: "1048576".to_string(),
        segment_count: 6,
        max_connections_per_host: 8,
        system_notifications: true,
        close_to_tray: false,
        start_on_boot: false,
        auto_resume_on_startup: false,
        floating_window_enabled: false,
        clipboard_monitor_enabled: true,
        accent_color: AppAccentColor::Blue,
        proxy_mode: tauri_app_lib::proxy::AppProxyMode::Off,
        proxy_url: String::new(),
        proxy_no_proxy: String::new(),
        proxy_username: String::new(),
        proxy_password_saved: false,
        schedule_download_window_enabled: false,
        schedule_download_window_start: "00:00".to_string(),
        schedule_download_window_end: "06:00".to_string(),
        schedule_speed_limit_window_enabled: false,
        schedule_speed_limit_window_start: "18:00".to_string(),
        schedule_speed_limit_window_end: "23:00".to_string(),
        schedule_speed_limit_bps: None,
        titlebar_gradient_enabled: true,
        completion_action: CompletionAction::None,
        completion_countdown_seconds: 30,
        completion_run_command: String::new(),
        delete_to_trash: true,
        auto_update_check_enabled: true,
        ffmpeg_path: None,
        bt_upload_limit_bps: None,
    };

    let segments = db::ensure_task_segments_with_settings(&pool, &task, &settings)
        .await
        .expect("segments");

    assert_eq!(segments.len(), 6);
    assert_eq!(segments[0].range_start, 0);
    assert_eq!(
        segments.last().expect("last").range_end,
        task.total_size - 1
    );
    for window in segments.windows(2) {
        assert_eq!(window[0].range_end + 1, window[1].range_start);
    }
}

#[tokio::test]
async fn ftp_task_creates_single_rest_segment_but_reserves_dynamic_slots() {
    let pool = test_pool("ftp-planning").await;
    let total_size = db::DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES * 4;
    let mut task = sample_task("task-ftp", total_size);
    task.url = "ftp://user:password@example.com/file.bin".to_string();
    task.final_url = Some(task.url.clone());
    task.protocol = "ftp".to_string();
    task.source_key = "ftp://example.com:21".to_string();
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert ftp task");

    let settings = AppSettings {
        max_active_tasks: 2,
        default_save_dir: "C:\\Downloads".to_string(),
        global_speed_limit_bps: None,
        multi_connection_threshold_bytes: db::DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES.to_string(),
        segment_count: 8,
        max_connections_per_host: 8,
        system_notifications: true,
        close_to_tray: false,
        start_on_boot: false,
        auto_resume_on_startup: false,
        floating_window_enabled: false,
        clipboard_monitor_enabled: true,
        accent_color: AppAccentColor::Blue,
        proxy_mode: tauri_app_lib::proxy::AppProxyMode::Off,
        proxy_url: String::new(),
        proxy_no_proxy: String::new(),
        proxy_username: String::new(),
        proxy_password_saved: false,
        schedule_download_window_enabled: false,
        schedule_download_window_start: "00:00".to_string(),
        schedule_download_window_end: "06:00".to_string(),
        schedule_speed_limit_window_enabled: false,
        schedule_speed_limit_window_start: "18:00".to_string(),
        schedule_speed_limit_window_end: "23:00".to_string(),
        schedule_speed_limit_bps: None,
        titlebar_gradient_enabled: true,
        completion_action: CompletionAction::None,
        completion_countdown_seconds: 30,
        completion_run_command: String::new(),
        delete_to_trash: true,
        auto_update_check_enabled: true,
        ffmpeg_path: None,
        bt_upload_limit_bps: None,
    };

    let planned_slots = db::planned_segment_count_with_plan(
        &task,
        db::DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES,
        settings.segment_count,
    );
    let segments = db::ensure_task_segments_with_settings(&pool, &task, &settings)
        .await
        .expect("ftp segments");

    assert_eq!(planned_slots, 4);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].unit_kind, "ftp_rest");
    assert_eq!(segments[0].range_start, 0);
    assert_eq!(segments[0].range_end, total_size - 1);
}

#[tokio::test]
async fn sftp_task_creates_single_file_work_unit() {
    let pool = test_pool("sftp-planning").await;
    let total_size = db::DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES * 4;
    let mut task = sample_task("task-sftp", total_size);
    task.url = "sftp://example.com/remote/file.bin".to_string();
    task.final_url = Some(task.url.clone());
    task.protocol = "sftp".to_string();
    task.source_key = "sftp://example.com:22".to_string();
    task.supports_parallel = false;
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert sftp task");

    let planned_slots = db::planned_segment_count_with_plan(
        &task,
        db::DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES,
        db::MAX_SEGMENT_COUNT,
    );
    let segments = db::ensure_task_segments(&pool, &task)
        .await
        .expect("sftp segments");

    assert_eq!(planned_slots, 1);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].unit_kind, "sftp_file");
    assert_eq!(segments[0].range_start, 0);
    assert_eq!(segments[0].range_end, total_size - 1);
}

#[tokio::test]
async fn sftp_parallel_task_creates_two_range_segments() {
    let pool = test_pool("sftp-parallel-planning").await;
    let total_size = db::DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES * 4;
    let mut task = sample_task("task-sftp-parallel", total_size);
    task.url = "sftp://example.com/remote/large.bin".to_string();
    task.final_url = Some(task.url.clone());
    task.protocol = "sftp".to_string();
    task.source_key = "sftp://example.com:22".to_string();
    task.supports_parallel = true;
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert sftp parallel task");

    let planned_slots = db::planned_segment_count_with_plan(
        &task,
        db::DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES,
        db::MAX_SEGMENT_COUNT,
    );
    let segments = db::ensure_task_segments(&pool, &task)
        .await
        .expect("sftp parallel segments");

    // SFTP is capped at SFTP_MAX_DYNAMIC_CONNECTIONS = 2.
    assert_eq!(planned_slots, 2);
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].unit_kind, "sftp_range");
    assert_eq!(segments[1].unit_kind, "sftp_range");
    assert_eq!(segments[0].range_start, 0);
    assert_eq!(segments[0].range_end + 1, segments[1].range_start);
    assert_eq!(segments[1].range_end, total_size - 1);
}

#[tokio::test]
async fn sftp_parallel_task_below_threshold_keeps_single_segment() {
    let pool = test_pool("sftp-below-threshold").await;
    let total_size = db::DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES / 2;
    let mut task = sample_task("task-sftp-small", total_size);
    task.url = "sftp://example.com/remote/small.bin".to_string();
    task.final_url = Some(task.url.clone());
    task.protocol = "sftp".to_string();
    task.source_key = "sftp://example.com:22".to_string();
    task.supports_parallel = true;
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert sftp small task");

    let planned_slots = db::planned_segment_count_with_plan(
        &task,
        db::DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES,
        db::MAX_SEGMENT_COUNT,
    );
    let segments = db::ensure_task_segments(&pool, &task)
        .await
        .expect("sftp small segments");

    assert_eq!(planned_slots, 1);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].unit_kind, "sftp_file");
}

#[tokio::test]
async fn bt_task_creates_single_piece_work_unit() {
    let pool = test_pool("bt-planning").await;
    let mut task = sample_task("task-bt", 0);
    task.url = "magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10&dn=Sintel".to_string();
    task.final_url = Some("bt:08ada5a7a6183aae1e09d831df6748d566095a10".to_string());
    task.protocol = "bt".to_string();
    task.task_kind = TaskKind::MultiFile;
    task.source_key = "bt:08ada5a7a6183aae1e09d831df6748d566095a10".to_string();
    task.supports_parallel = true;
    task.supports_multi_file = true;
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert bt task");

    let planned_slots = db::planned_segment_count_with_plan(
        &task,
        db::DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES,
        db::MAX_SEGMENT_COUNT,
    );
    let segments = db::ensure_task_segments(&pool, &task)
        .await
        .expect("bt segments");

    assert_eq!(planned_slots, 1);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].unit_kind, "bt_piece");
    assert_eq!(segments[0].range_start, 0);
    assert_eq!(segments[0].range_end, 0);
}

#[tokio::test]
async fn splitting_largest_remaining_segment_keeps_ranges_contiguous() {
    let pool = test_pool("split-segment").await;
    let total_size = db::DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES + 7;
    let task = sample_task("task-split", total_size);
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");
    let segments = db::ensure_task_segments(&pool, &task)
        .await
        .expect("segments");

    db::update_segment_progress(
        &pool,
        &segments[0].id,
        segments[0].range_start + 1024,
        SegmentStatus::Downloading,
    )
    .await
    .expect("progress");

    let split =
        db::split_largest_remaining_segment(&pool, &task.id, 1024, db::MAX_AUTO_SEGMENT_COUNT)
            .await
            .expect("split")
            .expect("split result");
    let next = db::list_segment_records(&pool, &task.id)
        .await
        .expect("list segments");

    assert_eq!(next.len(), 5);
    assert_eq!(split.original_range_end + 1, split.tail_segment.range_start);
    assert_eq!(next[0].range_start, 0);
    assert_eq!(next.last().expect("last").range_end, total_size - 1);
    for window in next.windows(2) {
        assert_eq!(window[0].range_end + 1, window[1].range_start);
    }
}

#[tokio::test]
async fn progress_updates_task_and_segment_together() {
    let pool = test_pool("progress-segment").await;
    let task = sample_task("task-progress", 100);
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");
    let segment = db::ensure_single_segment_for_task(&pool, &task)
        .await
        .expect("segment");

    db::update_task_and_segment_progress(
        &pool,
        &task.id,
        &segment.id,
        40,
        2048,
        1,
        TaskStatus::Downloading,
    )
    .await
    .expect("progress");

    let task = db::get_task_record(&pool, &task.id)
        .await
        .expect("load task")
        .expect("task");
    let segment = db::get_first_segment_record(&pool, &task.id)
        .await
        .expect("load segment")
        .expect("segment");

    assert_eq!(task.downloaded_bytes, 40);
    assert_eq!(task.speed_bps, 2048);
    assert_eq!(segment.downloaded_until, 40);
    assert_eq!(segment.status, SegmentStatus::Downloading);
}

#[tokio::test]
async fn settings_defaults_use_download_dir_and_two_active_tasks() {
    let pool = test_pool("settings-defaults").await;
    let settings = db::get_settings(&pool, "C:\\Downloads".to_string())
        .await
        .expect("settings");

    assert_eq!(settings.max_active_tasks, 2);
    assert_eq!(settings.default_save_dir, "C:\\Downloads");
    assert!(settings.global_speed_limit_bps.is_none());
    assert_eq!(
        settings.multi_connection_threshold_bytes,
        db::DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES.to_string()
    );
    assert_eq!(settings.segment_count, db::DEFAULT_SEGMENT_COUNT);
    assert_eq!(
        settings.max_connections_per_host,
        db::DEFAULT_MAX_CONNECTIONS_PER_HOST
    );
    assert!(!settings.floating_window_enabled);
    assert!(settings.clipboard_monitor_enabled);
    assert!(!settings.auto_resume_on_startup);
    assert_eq!(settings.proxy_mode, tauri_app_lib::proxy::AppProxyMode::Off);
    assert!(settings.proxy_url.is_empty());
    assert!(settings.proxy_no_proxy.is_empty());
    assert!(settings.proxy_username.is_empty());
    assert!(!settings.proxy_password_saved);
}

#[tokio::test]
async fn settings_upsert_and_clamp_active_task_count() {
    let pool = test_pool("settings-upsert").await;
    db::upsert_settings(
        &pool,
        &AppSettings {
            max_active_tasks: 5,
            default_save_dir: "D:\\Vibe".to_string(),
            global_speed_limit_bps: None,
            multi_connection_threshold_bytes: "1048576".to_string(),
            segment_count: 12,
            max_connections_per_host: 12,
            system_notifications: true,
            close_to_tray: false,
            start_on_boot: false,
            auto_resume_on_startup: true,
            floating_window_enabled: false,
            clipboard_monitor_enabled: true,
            accent_color: AppAccentColor::Blue,
            proxy_mode: tauri_app_lib::proxy::AppProxyMode::Off,
            proxy_url: String::new(),
            proxy_no_proxy: String::new(),
            proxy_username: String::new(),
            proxy_password_saved: false,
            schedule_download_window_enabled: false,
            schedule_download_window_start: "00:00".to_string(),
            schedule_download_window_end: "06:00".to_string(),
            schedule_speed_limit_window_enabled: false,
            schedule_speed_limit_window_start: "18:00".to_string(),
            schedule_speed_limit_window_end: "23:00".to_string(),
            schedule_speed_limit_bps: None,
            titlebar_gradient_enabled: true,
            completion_action: CompletionAction::None,
            completion_countdown_seconds: 30,
            completion_run_command: String::new(),
            delete_to_trash: true,
            auto_update_check_enabled: true,
            ffmpeg_path: None,
            bt_upload_limit_bps: None,
        },
    )
    .await
    .expect("upsert settings");

    let settings = db::get_settings(&pool, "C:\\Downloads".to_string())
        .await
        .expect("settings");
    assert_eq!(settings.max_active_tasks, 5);
    assert_eq!(settings.default_save_dir, "D:\\Vibe");
    assert_eq!(settings.multi_connection_threshold_bytes, "1048576");
    assert_eq!(settings.segment_count, db::MAX_SEGMENT_COUNT);
    assert_eq!(settings.max_connections_per_host, 12);
    assert!(settings.clipboard_monitor_enabled);

    db::upsert_settings(
        &pool,
        &AppSettings {
            max_active_tasks: 99,
            default_save_dir: "D:\\Vibe".to_string(),
            global_speed_limit_bps: Some("2048".to_string()),
            multi_connection_threshold_bytes: "0".to_string(),
            segment_count: 999,
            max_connections_per_host: 999,
            system_notifications: false,
            close_to_tray: true,
            start_on_boot: true,
            auto_resume_on_startup: false,
            floating_window_enabled: true,
            clipboard_monitor_enabled: false,
            accent_color: AppAccentColor::Blue,
            proxy_mode: tauri_app_lib::proxy::AppProxyMode::Off,
            proxy_url: String::new(),
            proxy_no_proxy: String::new(),
            proxy_username: String::new(),
            proxy_password_saved: false,
            schedule_download_window_enabled: true,
            schedule_download_window_start: "22:00".to_string(),
            schedule_download_window_end: "06:00".to_string(),
            schedule_speed_limit_window_enabled: true,
            schedule_speed_limit_window_start: "18:00".to_string(),
            schedule_speed_limit_window_end: "23:00".to_string(),
            schedule_speed_limit_bps: Some("1024".to_string()),
            titlebar_gradient_enabled: false,
            completion_action: CompletionAction::ExitApp,
            completion_countdown_seconds: 45,
            completion_run_command: String::new(),
            delete_to_trash: true,
            auto_update_check_enabled: true,
            ffmpeg_path: None,
            bt_upload_limit_bps: None,
        },
    )
    .await
    .expect("upsert settings");
    let settings = db::get_settings(&pool, "C:\\Downloads".to_string())
        .await
        .expect("settings");
    assert_eq!(settings.max_active_tasks, db::MAX_MAX_ACTIVE_TASKS);
    assert_eq!(settings.global_speed_limit_bps.as_deref(), Some("2048"));
    assert_eq!(settings.multi_connection_threshold_bytes, "0");
    assert_eq!(settings.segment_count, db::MAX_SEGMENT_COUNT);
    assert_eq!(
        settings.max_connections_per_host,
        db::MAX_MAX_CONNECTIONS_PER_HOST
    );
    assert!(!settings.clipboard_monitor_enabled);
}

#[tokio::test]
async fn recovery_target_update_preserves_progress_and_retargets_temp_path() {
    let pool = test_pool("recovery-target").await;
    let mut task = sample_task("task-recovery-target", 1024);
    task.downloaded_bytes = 512;
    task.status = TaskStatus::NeedsAttention;
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");

    let save_dir = std::env::temp_dir().join("vibe-recovery-target");
    let final_path = save_dir.join("renamed.bin");
    // ARC-02: retargeting rewrites temp to `{final}.{task_id}.vibe-downloading`
    // so concurrent same-name recoveries cannot collide on a shared temp file.
    let expected_temp_path = format!(
        "{}.{}.vibe-downloading",
        final_path.to_string_lossy(),
        task.id
    );
    db::update_task_save_target(
        &pool,
        &task.id,
        "renamed.bin",
        &save_dir.to_string_lossy(),
        &final_path.to_string_lossy(),
    )
    .await
    .expect("update target");

    let updated = db::get_task_record(&pool, &task.id)
        .await
        .expect("load task")
        .expect("task");
    assert_eq!(updated.file_name, "renamed.bin");
    assert_eq!(updated.save_dir, save_dir.to_string_lossy());
    assert_eq!(
        updated.final_path.as_deref(),
        Some(final_path.to_string_lossy().as_ref())
    );
    assert_eq!(
        updated.temp_path.as_deref(),
        Some(expected_temp_path.as_str())
    );
    assert_eq!(updated.downloaded_bytes, 512);
}

#[tokio::test]
async fn restart_reset_clears_progress_and_segments() {
    let pool = test_pool("restart-reset").await;
    let mut task = sample_task("task-restart-reset", 4096);
    task.downloaded_bytes = 2048;
    task.speed_bps = 999;
    task.connection_count = 2;
    task.status = TaskStatus::NeedsAttention;
    task.error_message =
        Some("Remote file changed. Restart download to avoid corruption.".to_string());
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");
    db::ensure_task_segments(&pool, &task)
        .await
        .expect("segments");

    db::delete_segments_for_task(&pool, &task.id)
        .await
        .expect("delete segments");
    db::reset_task_download_state(&pool, &task.id)
        .await
        .expect("reset task");

    let updated = db::get_task_record(&pool, &task.id)
        .await
        .expect("load task")
        .expect("task");
    let segments = db::list_segment_records(&pool, &task.id)
        .await
        .expect("segments");
    assert!(segments.is_empty());
    assert_eq!(updated.status, TaskStatus::Queued);
    assert_eq!(updated.downloaded_bytes, 0);
    assert_eq!(updated.speed_bps, 0);
    assert_eq!(updated.connection_count, 0);
    assert_eq!(updated.health_summary.as_deref(), Some("Queued"));
    assert!(updated.error_message.is_none());
}

#[test]
fn task_updated_payload_serializes() {
    let task = Task::from(sample_task("task-updated-payload", 128));
    let payload = TaskUpdatedPayload { task };
    let json = serde_json::to_value(payload).expect("serialize");
    assert_eq!(json["task"]["id"], "task-updated-payload");
}

#[tokio::test]
async fn browser_messages_track_duplicates_and_latest_error() {
    let pool = test_pool("browser-messages").await;
    assert!(!db::browser_message_exists(&pool, "request-1")
        .await
        .expect("exists"));

    db::insert_browser_message(
        &pool,
        "request-1",
        BrowserKind::Chrome,
        "https://example.com/file.zip",
        "received",
        None,
    )
    .await
    .expect("insert message");
    assert!(db::browser_message_exists(&pool, "request-1")
        .await
        .expect("exists"));

    db::update_browser_message_status(
        &pool,
        "request-1",
        "failed",
        Some("Browser handoff URL is invalid."),
    )
    .await
    .expect("update message");
    let latest = db::latest_browser_error(&pool, BrowserKind::Chrome)
        .await
        .expect("latest error");
    assert_eq!(latest.as_deref(), Some("Browser handoff URL is invalid."));
}

#[tokio::test]
async fn queued_task_query_uses_fifo_order() {
    let pool = test_pool("queued-fifo").await;
    let mut first = sample_task("task-first", 100);
    first.created_at = "2024-01-01T00:00:00Z".to_string();
    first.updated_at = first.created_at.clone();
    let mut second = sample_task("task-second", 100);
    second.created_at = "2024-01-02T00:00:00Z".to_string();
    second.updated_at = second.created_at.clone();
    let mut paused = sample_task("task-paused", 100);
    paused.status = TaskStatus::Paused;

    for task in [&second, &paused, &first] {
        db::insert_task_record(&pool, task)
            .await
            .expect("insert task");
    }

    let queued = db::list_queued_task_records(&pool, 2)
        .await
        .expect("queued");
    assert_eq!(
        queued
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        vec!["task-first", "task-second"]
    );
}

#[tokio::test]
async fn cursor_task_query_pages_and_maps_failure_categories() {
    let pool = test_pool("cursor-tasks").await;
    for index in 0..12 {
        let mut task = sample_task(&format!("task-cursor-{index:02}"), 100);
        task.updated_at = format!("2024-01-01T00:{index:02}:00Z");
        task.created_at = task.updated_at.clone();
        task.source_key = if index % 2 == 0 {
            format!("source-a-{}", index)
        } else {
            format!("source-b-{}", index)
        };
        if index == 3 || index == 7 {
            task.status = TaskStatus::NeedsAttention;
            task.error_code = Some("auth_headers_expired".to_string());
        }
        db::insert_task_record(&pool, &task)
            .await
            .expect("insert cursor task");
    }

    let first_query = db::TaskListQuery {
        nav: "all".to_string(),
        search: String::new(),
        sort_key: "updated_at".to_string(),
        sort_direction: "desc".to_string(),
        file_type: "all".to_string(),
        source: "all".to_string(),
        failure: "all".to_string(),
        resume: "all".to_string(),
        page: 0,
        page_size: 5,
        cursor_value: None,
        cursor_id: None,
    };
    let first = db::list_task_records_cursor(&pool, &first_query)
        .await
        .expect("first cursor page");
    assert_eq!(first.items.len(), 5);
    assert!(first.has_more);
    // Cursor pages expose only a lower bound so the first-screen path avoids
    // a COUNT query over the filtered task set.
    assert_eq!(first.total, 6);
    assert_eq!(first.items[0].id, "task-cursor-11");

    let last = first.items.last().expect("last first page");
    let second = db::list_task_records_cursor(
        &pool,
        &db::TaskListQuery {
            cursor_value: Some(last.updated_at.clone()),
            cursor_id: Some(last.id.clone()),
            ..first_query.clone()
        },
    )
    .await
    .expect("second cursor page");
    assert_eq!(second.items[0].id, "task-cursor-06");
    assert!(!second.items.iter().any(|task| task.id == last.id));

    let auth = db::list_task_records_cursor(
        &pool,
        &db::TaskListQuery {
            failure: "auth".to_string(),
            page_size: 10,
            cursor_value: None,
            cursor_id: None,
            ..first_query.clone()
        },
    )
    .await
    .expect("auth category page");
    assert_eq!(auth.items.len(), 2);
    assert!(auth
        .items
        .iter()
        .all(|task| task.error_code.as_deref() == Some("auth_headers_expired")));

    let mut sftp_task = sample_task("task-sftp-failure", 100);
    sftp_task.updated_at = "2024-01-01T00:20:00Z".to_string();
    sftp_task.created_at = sftp_task.updated_at.clone();
    sftp_task.status = TaskStatus::Failed;
    sftp_task.error_code = Some("sftp_host_key_changed".to_string());
    db::insert_task_record(&pool, &sftp_task)
        .await
        .expect("insert sftp failure task");
    let sftp = db::list_task_records_cursor(
        &pool,
        &db::TaskListQuery {
            failure: "sftp".to_string(),
            page_size: 10,
            cursor_value: None,
            cursor_id: None,
            ..first_query
        },
    )
    .await
    .expect("sftp category page");
    assert_eq!(sftp.items.len(), 1);
    assert_eq!(
        sftp.items[0].error_code.as_deref(),
        Some("sftp_host_key_changed")
    );
}

#[tokio::test]
async fn task_query_indexes_are_created() {
    let pool = test_pool("task-query-indexes").await;
    let indexes: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT name
        FROM sqlite_master
        WHERE type = 'index'
          AND name IN (
              'idx_tasks_updated_at_id',
              'idx_tasks_created_at_id',
              'idx_tasks_status_updated_at_id',
              'idx_tasks_source_key_updated_at_id',
              'idx_tasks_error_code_updated_at_id'
          )
        "#,
    )
    .fetch_all(&pool)
    .await
    .expect("query indexes");

    for expected in [
        "idx_tasks_updated_at_id",
        "idx_tasks_created_at_id",
        "idx_tasks_status_updated_at_id",
        "idx_tasks_source_key_updated_at_id",
        "idx_tasks_error_code_updated_at_id",
    ] {
        assert!(indexes.iter().any(|index| index == expected), "{expected}");
    }
}

#[tokio::test]
async fn task_stats_snapshot_uses_full_database_counts() {
    let pool = test_pool("task-stats").await;
    let mut downloading = sample_task("stats-downloading", 1_000);
    downloading.status = TaskStatus::Downloading;
    downloading.downloaded_bytes = 250;
    downloading.speed_bps = 40;
    db::insert_task_record(&pool, &downloading)
        .await
        .expect("insert downloading");

    let mut retrying = sample_task("stats-retrying", 2_000);
    retrying.status = TaskStatus::Retrying;
    retrying.downloaded_bytes = 500;
    retrying.speed_bps = 60;
    db::insert_task_record(&pool, &retrying)
        .await
        .expect("insert retrying");

    for (id, status) in [
        ("stats-queued", TaskStatus::Queued),
        ("stats-paused", TaskStatus::Paused),
        ("stats-completed", TaskStatus::Completed),
        ("stats-failed", TaskStatus::Failed),
        ("stats-attention", TaskStatus::NeedsAttention),
    ] {
        let mut task = sample_task(id, 100);
        task.status = status;
        db::insert_task_record(&pool, &task)
            .await
            .expect("insert stats task");
    }

    let stats = db::task_stats_snapshot(&pool).await.expect("stats");
    assert_eq!(stats.all, "7");
    assert_eq!(stats.active, "2");
    assert_eq!(stats.queued, "1");
    assert_eq!(stats.attention, "1");
    assert_eq!(stats.paused, "1");
    assert_eq!(stats.completed, "1");
    assert_eq!(stats.failed, "1");
    assert_eq!(stats.total_speed, "100");
    assert_eq!(stats.total_downloaded, "750");
    assert_eq!(stats.total_bytes, "3000");
    assert_eq!(stats.featured_task_id.as_deref(), Some("stats-retrying"));
}

#[tokio::test]
async fn browser_realtime_task_query_returns_active_plus_recent() {
    let pool = test_pool("browser-realtime-query").await;
    for index in 0..60 {
        let mut task = sample_task(&format!("browser-recent-{index:02}"), 100);
        task.status = TaskStatus::Completed;
        task.updated_at = format!("2024-01-01T00:{index:02}:00Z");
        task.created_at = task.updated_at.clone();
        db::insert_task_record(&pool, &task)
            .await
            .expect("insert recent");
    }

    for (id, status) in [
        ("browser-active-downloading", TaskStatus::Downloading),
        ("browser-active-attention", TaskStatus::NeedsAttention),
    ] {
        let mut task = sample_task(id, 100);
        task.status = status;
        task.updated_at = "2024-01-02T00:00:00Z".to_string();
        task.created_at = task.updated_at.clone();
        db::insert_task_record(&pool, &task)
            .await
            .expect("insert active");
    }

    let records = db::list_browser_realtime_task_records(&pool)
        .await
        .expect("browser realtime records");
    assert_eq!(records.len(), 52);
    assert!(records
        .iter()
        .any(|task| task.id == "browser-active-downloading"));
    assert!(records
        .iter()
        .any(|task| task.id == "browser-active-attention"));
    assert!(records.iter().any(|task| task.id == "browser-recent-59"));
    assert!(!records.iter().any(|task| task.id == "browser-recent-00"));
}

#[tokio::test]
async fn reset_interrupted_tasks_pauses_active_records() {
    let pool = test_pool("reset-interrupted").await;
    let mut downloading = sample_task("task-downloading", 100);
    downloading.status = TaskStatus::Downloading;
    let mut retrying = sample_task("task-retrying", 100);
    retrying.status = TaskStatus::Retrying;
    let queued = sample_task("task-queued", 100);

    for task in [&downloading, &retrying, &queued] {
        db::insert_task_record(&pool, task)
            .await
            .expect("insert task");
    }

    db::reset_interrupted_tasks(&pool, false)
        .await
        .expect("reset interrupted");

    assert_eq!(
        db::get_task_record(&pool, "task-downloading")
            .await
            .expect("load")
            .expect("task")
            .status,
        TaskStatus::Paused
    );
    assert_eq!(
        db::get_task_record(&pool, "task-retrying")
            .await
            .expect("load")
            .expect("task")
            .status,
        TaskStatus::Paused
    );
    assert_eq!(
        db::get_task_record(&pool, "task-queued")
            .await
            .expect("load")
            .expect("task")
            .status,
        TaskStatus::Queued
    );
}

#[tokio::test]
async fn reset_interrupted_tasks_can_queue_active_records_for_startup_resume() {
    let pool = test_pool("reset-interrupted-auto-resume").await;
    let mut downloading = sample_task("task-downloading", 100);
    downloading.status = TaskStatus::Downloading;
    let mut retrying = sample_task("task-retrying", 100);
    retrying.status = TaskStatus::Retrying;
    let mut paused = sample_task("task-paused", 100);
    paused.status = TaskStatus::Paused;

    for task in [&downloading, &retrying, &paused] {
        db::insert_task_record(&pool, task)
            .await
            .expect("insert task");
    }

    db::reset_interrupted_tasks(&pool, true)
        .await
        .expect("reset interrupted");

    assert_eq!(
        db::get_task_record(&pool, "task-downloading")
            .await
            .expect("load")
            .expect("task")
            .status,
        TaskStatus::Queued
    );
    assert_eq!(
        db::get_task_record(&pool, "task-retrying")
            .await
            .expect("load")
            .expect("task")
            .status,
        TaskStatus::Queued
    );
    assert_eq!(
        db::get_task_record(&pool, "task-paused")
            .await
            .expect("load")
            .expect("task")
            .status,
        TaskStatus::Paused
    );
}

#[tokio::test]
async fn non_http_protocols_share_process_restart_recovery_contract() {
    const PROTOCOLS: [&str; 7] = ["ftp", "sftp", "bt", "hls", "dash", "webdav", "metalink"];

    for auto_resume in [false, true] {
        let pool = test_pool(if auto_resume {
            "protocol-restart-auto"
        } else {
            "protocol-restart-paused"
        })
        .await;
        for (index, protocol) in PROTOCOLS.iter().enumerate() {
            let mut task = sample_task(&format!("{protocol}-restart"), 1024);
            task.protocol = (*protocol).to_string();
            task.status = if index % 2 == 0 {
                TaskStatus::Downloading
            } else {
                TaskStatus::Retrying
            };
            task.downloaded_bytes = 512;
            db::insert_task_record(&pool, &task)
                .await
                .expect("insert protocol task");
        }

        db::reset_interrupted_tasks(&pool, auto_resume)
            .await
            .expect("reset interrupted protocol tasks");

        let expected = if auto_resume {
            TaskStatus::Queued
        } else {
            TaskStatus::Paused
        };
        for protocol in PROTOCOLS {
            let task = db::get_task_record(&pool, &format!("{protocol}-restart"))
                .await
                .expect("load protocol task")
                .expect("protocol task");
            assert_eq!(
                task.status, expected,
                "unexpected restart state for {protocol}"
            );
            assert_eq!(
                task.downloaded_bytes, 512,
                "restart recovery must preserve progress for {protocol}"
            );
        }
    }
}

#[tokio::test]
async fn non_http_protocols_share_explicit_restart_reset_contract() {
    const PROTOCOLS: [&str; 7] = ["ftp", "sftp", "bt", "hls", "dash", "webdav", "metalink"];
    let pool = test_pool("protocol-explicit-restart").await;

    for protocol in PROTOCOLS {
        let mut task = sample_task(&format!("{protocol}-explicit-restart"), 1024);
        task.protocol = protocol.to_string();
        task.status = TaskStatus::NeedsAttention;
        task.downloaded_bytes = 512;
        task.speed_bps = 128;
        task.connection_count = 2;
        task.error_message = Some("Restart required.".to_string());
        db::insert_task_record(&pool, &task)
            .await
            .expect("insert protocol task");

        db::reset_task_download_state(&pool, &task.id)
            .await
            .expect("reset protocol task");
        let reset = db::get_task_record(&pool, &task.id)
            .await
            .expect("load reset task")
            .expect("reset task");
        assert_eq!(
            reset.status,
            TaskStatus::Queued,
            "unexpected reset state for {protocol}"
        );
        assert_eq!(
            reset.downloaded_bytes, 0,
            "restart must clear progress for {protocol}"
        );
        assert_eq!(reset.speed_bps, 0);
        assert_eq!(reset.connection_count, 0);
        assert!(reset.error_message.is_none());
    }
}

#[tokio::test]
async fn non_http_protocols_share_cancellation_state_contract() {
    const PROTOCOLS: [&str; 7] = ["ftp", "sftp", "bt", "hls", "dash", "webdav", "metalink"];
    let pool = test_pool("protocol-cancel-state").await;

    for protocol in PROTOCOLS {
        let mut task = sample_task(&format!("{protocol}-cancel"), 1024);
        task.protocol = protocol.to_string();
        task.status = TaskStatus::Downloading;
        task.downloaded_bytes = 512;
        task.speed_bps = 128;
        task.connection_count = 2;
        db::insert_task_record(&pool, &task)
            .await
            .expect("insert protocol task");

        let canceled = db::update_task_status(
            &pool,
            &task.id,
            TaskStatus::Failed,
            Some(TaskStatus::Downloading),
            0,
            0,
            Some("Canceled by user."),
            Some("Canceled by user."),
        )
        .await
        .expect("cancel protocol task")
        .expect("updated protocol task");
        assert_eq!(
            canceled.status,
            TaskStatus::Failed,
            "unexpected cancel state for {protocol}"
        );
        assert_eq!(
            canceled.downloaded_bytes, 512,
            "cancel must preserve resumable progress for {protocol}"
        );
        assert_eq!(canceled.speed_bps, 0);
        assert_eq!(canceled.connection_count, 0);
    }
}

#[tokio::test]
async fn non_http_protocols_share_atomic_pause_resume_persistence_contract() {
    const PROTOCOLS: [&str; 7] = ["ftp", "sftp", "bt", "hls", "dash", "webdav", "metalink"];
    let pool = test_pool("protocol-pause-resume").await;
    let root = std::env::temp_dir().join(format!(
        "vibe-protocol-pause-resume-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("create lifecycle fixture directory");

    for protocol in PROTOCOLS {
        let mut task = sample_task(&format!("{protocol}-pause-resume"), 1024);
        task.protocol = protocol.to_string();
        task.status = TaskStatus::Downloading;
        task.downloaded_bytes = 512;
        task.speed_bps = 128;
        task.connection_count = 2;
        let temp_path = root.join(format!("{protocol}.vibe-downloading"));
        let final_path = root.join(format!("{protocol}.bin"));
        if matches!(protocol, "hls" | "dash") {
            std::fs::create_dir_all(&temp_path).expect("create media staging directory");
        } else {
            std::fs::write(&temp_path, vec![0xA5; 512]).expect("write resumable temp file");
        }
        task.temp_path = Some(temp_path.to_string_lossy().to_string());
        task.final_path = Some(final_path.to_string_lossy().to_string());
        db::insert_task_record(&pool, &task)
            .await
            .expect("insert protocol task");
        let segment = db::ensure_single_segment_for_task(&pool, &task)
            .await
            .expect("create protocol work unit");
        db::update_segment_runtime_progress(
            &pool,
            &segment.id,
            512,
            128,
            SegmentStatus::Downloading,
        )
        .await
        .expect("seed protocol progress");
        db::update_task_retry_after(&pool, &task.id, Some("2099-01-01T00:00:00Z"))
            .await
            .expect("seed retry schedule");

        let mut pause_tx = pool.begin().await.expect("begin pause transaction");
        db::update_task_status_in_tx(
            &mut pause_tx,
            &task.id,
            TaskStatus::Paused,
            Some(TaskStatus::Downloading),
            0,
            0,
            Some("Paused"),
            None,
        )
        .await
        .expect("pause task")
        .expect("pause transition matched");
        db::update_segments_status_for_task_in_tx(
            &mut pause_tx,
            &task.id,
            SegmentStatus::Pending,
            None,
        )
        .await
        .expect("pause work units");
        db::update_task_retry_after_in_tx(&mut pause_tx, &task.id, None)
            .await
            .expect("clear retry schedule");
        db::insert_task_event_in_tx(&mut pause_tx, &task.id, "paused", None)
            .await
            .expect("record pause event");
        pause_tx.commit().await.expect("commit pause transaction");

        let paused = db::get_task_record(&pool, &task.id)
            .await
            .expect("load paused task")
            .expect("paused task");
        let paused_segments = db::list_segment_records(&pool, &task.id)
            .await
            .expect("load paused work units");
        assert_eq!(
            paused.status,
            TaskStatus::Paused,
            "pause failed for {protocol}"
        );
        assert_eq!(paused.downloaded_bytes, 512);
        assert_eq!(paused.speed_bps, 0);
        assert_eq!(paused.connection_count, 0);
        assert!(paused.retry_after_at.is_none());
        assert_eq!(paused_segments[0].status, SegmentStatus::Pending);
        assert_eq!(paused_segments[0].downloaded_until, 512);
        assert_eq!(paused_segments[0].speed_bps, 0);
        assert!(
            temp_path.exists(),
            "pause removed temp storage for {protocol}"
        );
        assert!(
            !final_path.exists(),
            "pause published a final file for {protocol}"
        );

        let mut resume_tx = pool.begin().await.expect("begin resume transaction");
        db::update_task_status_in_tx(
            &mut resume_tx,
            &task.id,
            TaskStatus::Queued,
            Some(TaskStatus::Paused),
            0,
            0,
            Some("Queued"),
            None,
        )
        .await
        .expect("resume task")
        .expect("resume transition matched");
        db::update_segments_status_for_task_in_tx(
            &mut resume_tx,
            &task.id,
            SegmentStatus::Pending,
            None,
        )
        .await
        .expect("resume work units");
        db::update_task_retry_after_in_tx(&mut resume_tx, &task.id, None)
            .await
            .expect("keep retry schedule clear");
        db::insert_task_event_in_tx(&mut resume_tx, &task.id, "resumed", None)
            .await
            .expect("record resume event");
        resume_tx.commit().await.expect("commit resume transaction");

        let resumed = db::get_task_record(&pool, &task.id)
            .await
            .expect("load resumed task")
            .expect("resumed task");
        assert_eq!(
            resumed.status,
            TaskStatus::Queued,
            "resume failed for {protocol}"
        );
        assert_eq!(resumed.downloaded_bytes, 512);
        assert!(
            temp_path.exists(),
            "resume removed temp storage for {protocol}"
        );
        assert!(
            !final_path.exists(),
            "resume published a final file for {protocol}"
        );
    }

    pool.close().await;
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn remote_metadata_change_blocks_resume() {
    let mut task = sample_task("task-remote", 100);
    task.etag = Some("etag-a".to_string());
    task.last_modified = Some("Mon, 01 Jan 2024 00:00:00 GMT".to_string());

    let mut probe = sample_probe(100);
    assert!(resume_mismatch_message(&task, &probe).is_none());

    probe.etag = Some("etag-b".to_string());
    assert_eq!(
        resume_mismatch_message(&task, &probe).as_deref(),
        Some("Remote file changed. Restart download to avoid corruption.")
    );

    probe.etag = task.etag.clone();
    probe.last_modified = Some("Tue, 02 Jan 2024 00:00:00 GMT".to_string());
    assert_eq!(
        resume_mismatch_message(&task, &probe).as_deref(),
        Some("Remote file changed. Restart download to avoid corruption.")
    );
}

#[test]
fn local_resume_errors_are_explicit() {
    assert_eq!(
        local_resume_error(10, false, 0, 100, true),
        Some("Temporary file is missing. Restart this download.")
    );
    assert_eq!(
        local_resume_error(50, true, 40, 100, true),
        Some("Temporary file is smaller than the recorded progress.")
    );
    assert_eq!(
        local_resume_error(0, true, 120, 100, true),
        Some("Temporary file is larger than the remote file.")
    );
    assert_eq!(
        local_resume_error(0, true, 10, 100, false),
        Some("Resume unavailable. Restart this download from the beginning.")
    );
}

#[test]
fn segment_resume_errors_cover_multi_segment_corruption() {
    let task = sample_task(
        "task-corrupt-segments",
        db::DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES + 7,
    );
    let mut segments = db::planned_segments_for_task(&task);

    assert!(segment_resume_error(&segments, 0, true, 0, task.total_size, true).is_none());

    segments[1].downloaded_until = segments[1].range_end + 2;
    assert_eq!(
        segment_resume_error(&segments, 0, true, task.total_size, task.total_size, true),
        Some("Segment progress is outside its byte range. Restart this download.")
    );

    let mut segments = db::planned_segments_for_task(&task);
    segments[1].range_start += 1;
    assert_eq!(
        segment_resume_error(&segments, 0, true, task.total_size, task.total_size, true),
        Some("Segment records are inconsistent. Restart this download.")
    );

    let mut segments = db::planned_segments_for_task(&task);
    segments[2].downloaded_until = segments[2].range_start + 1024;
    assert_eq!(
        segment_resume_error(
            &segments,
            0,
            true,
            segments[2].range_start,
            task.total_size,
            true
        ),
        Some("Temporary file is smaller than the recorded progress.")
    );

    assert_eq!(
        segment_resume_error(&segments, 0, false, 0, task.total_size, true),
        Some("Temporary file is missing. Restart this download.")
    );
}

#[test]
fn multi_segment_remote_metadata_changes_are_blocked() {
    let mut task = sample_task(
        "task-remote-multi",
        db::DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES + 7,
    );
    task.etag = Some("etag-a".to_string());
    task.last_modified = Some("Mon, 01 Jan 2024 00:00:00 GMT".to_string());

    let mut probe = sample_probe(task.total_size);
    assert!(resume_mismatch_message(&task, &probe).is_none());

    probe.total_size += 1;
    assert_eq!(
        resume_mismatch_message(&task, &probe).as_deref(),
        Some("Remote file changed. Restart download to avoid corruption.")
    );

    let mut probe = sample_probe(task.total_size);
    probe.supports_resume = false;
    probe.supports_parallel = false;
    assert_eq!(
        resume_mismatch_message(&task, &probe).as_deref(),
        Some("Server no longer supports resume. Restart this download.")
    );
}

async fn test_pool(label: &str) -> sqlx::SqlitePool {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-db-{label}-{id}.sqlite"));
    db::connect(&path).await.expect("connect").pool
}

fn sample_task(id: &str, total_size: i64) -> TaskRecord {
    let now = tauri_app_lib::models::task::now_iso();
    TaskRecord {
        id: id.to_string(),
        url: "http://127.0.0.1/file".to_string(),
        final_url: Some("http://127.0.0.1/file".to_string()),
        protocol: "http".to_string(),
        task_kind: TaskKind::SingleFile,
        file_name: "file.bin".to_string(),
        save_dir: std::env::temp_dir().to_string_lossy().to_string(),
        temp_path: Some(
            std::env::temp_dir()
                .join(format!("{id}.vibe-downloading"))
                .to_string_lossy()
                .to_string(),
        ),
        final_path: Some(
            std::env::temp_dir()
                .join(format!("{id}.bin"))
                .to_string_lossy()
                .to_string(),
        ),
        total_size,
        downloaded_bytes: 0,
        status: TaskStatus::Queued,
        etag: Some("etag-a".to_string()),
        last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".to_string()),
        content_type: Some("application/octet-stream".to_string()),
        supports_resume: true,
        supports_parallel: true,
        supports_multi_file: false,
        source_key: format!("http://127.0.0.1/{}", id),
        connection_count: 0,
        speed_bps: 0,
        task_speed_limit_bps: None,
        priority: TaskPriority::Normal,
        queue_position: 0,
        category_key: None,
        obey_schedule: true,
        health_summary: Some("Queued".to_string()),
        error_message: None,
        error_code: None,
        recovery_actions: Vec::new(),
        retry_after_at: None,
        expected_hash_sha256: None,
        actual_hash_sha256: None,
        hash_status: HashVerificationStatus::NotRequested,
        hash_error: None,
        hash_verified_at: None,
        created_at: now.clone(),
        updated_at: now,
        files_version: 0,
    }
}

fn sample_probe(total_size: i64) -> ProbeResult {
    ProbeResult {
        final_url: "http://127.0.0.1/file".to_string(),
        file_name: "file.bin".to_string(),
        total_size,
        supports_resume: true,
        supports_parallel: true,
        supports_multi_file: false,
        source_key: "127.0.0.1".to_string(),
        etag: Some("etag-a".to_string()),
        last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".to_string()),
        content_type: Some("application/octet-stream".to_string()),
    }
}
