use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri_app_lib::{
    commands::tasks::{local_resume_error, resume_mismatch_message, segment_resume_error},
    db,
    download::ProbeResult,
    models::{
        AppSettings, BrowserKind, SegmentStatus, Task, TaskKind, TaskRecord, TaskStatus,
        TaskUpdatedPayload,
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

    db::upsert_settings(
        &pool,
        &AppSettings {
            max_active_tasks: 99,
            default_save_dir: "D:\\Vibe".to_string(),
            global_speed_limit_bps: Some("2048".to_string()),
            multi_connection_threshold_bytes: "0".to_string(),
            segment_count: 999,
            max_connections_per_host: 999,
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
}

#[tokio::test]
async fn recovery_target_update_preserves_progress_and_temp_path() {
    let pool = test_pool("recovery-target").await;
    let mut task = sample_task("task-recovery-target", 1024);
    task.downloaded_bytes = 512;
    task.status = TaskStatus::NeedsAttention;
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");
    let temp_path = task.temp_path.clone();

    let save_dir = std::env::temp_dir().join("vibe-recovery-target");
    let final_path = save_dir.join("renamed.bin");
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
    assert_eq!(updated.temp_path, temp_path);
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

    db::reset_interrupted_tasks(&pool)
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
    db::connect(&path).await.expect("connect")
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
            PathBuf::from(std::env::temp_dir())
                .join(format!("{id}.vibe-downloading"))
                .to_string_lossy()
                .to_string(),
        ),
        final_path: Some(
            PathBuf::from(std::env::temp_dir())
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
        source_key: "127.0.0.1".to_string(),
        connection_count: 0,
        speed_bps: 0,
        health_summary: Some("Queued".to_string()),
        error_message: None,
        created_at: now.clone(),
        updated_at: now,
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
