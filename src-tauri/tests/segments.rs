use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri_app_lib::{
    commands::tasks::{local_resume_error, resume_mismatch_message},
    db,
    download::ProbeResult,
    models::{SegmentStatus, TaskRecord, TaskStatus},
};

#[tokio::test]
async fn ensure_single_segment_creates_task_range() {
    let pool = test_pool("create-segment").await;
    let task = sample_task("task-create", 100);
    db::insert_task_record(&pool, &task).await.expect("insert task");

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
    let small = sample_task("task-small", db::MULTI_CONNECTION_THRESHOLD_BYTES - 1);
    db::insert_task_record(&pool, &small).await.expect("insert small");
    let small_segments = db::ensure_task_segments(&pool, &small)
        .await
        .expect("small segments");
    assert_eq!(small_segments.len(), 1);

    let mut no_range = sample_task("task-no-range", db::MULTI_CONNECTION_THRESHOLD_BYTES);
    no_range.supports_range = false;
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
    let total_size = db::MULTI_CONNECTION_THRESHOLD_BYTES + 7;
    let task = sample_task("task-large", total_size);
    db::insert_task_record(&pool, &task).await.expect("insert task");

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
async fn progress_updates_task_and_segment_together() {
    let pool = test_pool("progress-segment").await;
    let task = sample_task("task-progress", 100);
    db::insert_task_record(&pool, &task).await.expect("insert task");
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
        supports_range: true,
        source_host: "127.0.0.1".to_string(),
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
        supports_range: true,
        source_host: "127.0.0.1".to_string(),
        etag: Some("etag-a".to_string()),
        last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".to_string()),
        content_type: Some("application/octet-stream".to_string()),
    }
}
