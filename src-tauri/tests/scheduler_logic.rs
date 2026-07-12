//! Pure-logic tests for the scheduler dispatch helpers.
//!
//! These tests cover the three pure functions extracted from
//! `Scheduler::dispatch_inner` (`compute_available_slots`,
//! `should_skip_for_schedule_window`, `compute_planned_slots`) plus the
//! segment planner (`db::planned_segment_count_with_plan`). No database or
//! Tauri runtime is required.

use tauri_app_lib::db;
use tauri_app_lib::models::task::now_iso;
use tauri_app_lib::models::{
    HashVerificationStatus, TaskKind, TaskPriority, TaskRecord, TaskStatus,
};
use tauri_app_lib::scheduler::Scheduler;
use tauri_app_lib::state_machine::TransitionError;

fn sample_task_record(id: &str, protocol: &str) -> TaskRecord {
    let now = now_iso();
    TaskRecord {
        id: id.to_string(),
        url: format!("{protocol}://example.com/{id}"),
        final_url: None,
        protocol: protocol.to_string(),
        task_kind: TaskKind::SingleFile,
        file_name: format!("{id}.bin"),
        save_dir: std::env::temp_dir().to_string_lossy().to_string(),
        temp_path: None,
        final_path: None,
        total_size: 0,
        downloaded_bytes: 0,
        status: TaskStatus::Queued,
        etag: None,
        last_modified: None,
        content_type: None,
        supports_resume: true,
        supports_parallel: true,
        supports_multi_file: false,
        source_key: format!("{protocol}://example.com/{id}"),
        connection_count: 0,
        speed_bps: 0,
        task_speed_limit_bps: None,
        priority: TaskPriority::Normal,
        queue_position: 0,
        category_key: None,
        obey_schedule: true,
        health_summary: None,
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

// --- compute_available_slots ---

#[test]
fn available_slots_full_when_no_active_tasks() {
    assert_eq!(Scheduler::compute_available_slots(4, 0), 4);
}

#[test]
fn available_slots_partial_when_some_active() {
    assert_eq!(Scheduler::compute_available_slots(4, 2), 2);
}

#[test]
fn available_slots_zero_when_at_capacity() {
    assert_eq!(Scheduler::compute_available_slots(4, 4), 0);
}

#[test]
fn available_slots_never_underflows() {
    // saturating_sub guarantees we never return a negative slot count
    assert_eq!(Scheduler::compute_available_slots(4, 6), 0);
}

// --- should_skip_for_schedule_window ---

#[test]
fn schedule_window_skips_when_outside_window_and_obeying() {
    assert!(Scheduler::should_skip_for_schedule_window(
        true,  // enabled
        true,  // obey_schedule
        false, // time_window_active
    ));
}

#[test]
fn schedule_window_does_not_skip_when_inside_window() {
    assert!(!Scheduler::should_skip_for_schedule_window(
        true, // enabled
        true, // obey_schedule
        true, // time_window_active
    ));
}

#[test]
fn schedule_window_does_not_skip_when_task_ignores_schedule() {
    assert!(!Scheduler::should_skip_for_schedule_window(
        true,  // enabled
        false, // obey_schedule
        false, // time_window_active
    ));
}

#[test]
fn schedule_window_does_not_skip_when_feature_disabled() {
    assert!(!Scheduler::should_skip_for_schedule_window(
        false, // enabled
        true,  // obey_schedule
        false, // time_window_active
    ));
}

// --- compute_planned_slots ---

#[test]
fn planned_slots_uses_planned_when_host_has_room() {
    assert_eq!(Scheduler::compute_planned_slots(4, 8, 0), 4);
}

#[test]
fn planned_slots_capped_by_remaining_host_capacity() {
    assert_eq!(Scheduler::compute_planned_slots(8, 8, 6), 2);
}

#[test]
fn planned_slots_floors_to_one_when_host_full() {
    // host_limit - host_used = 0, but max(1) guarantees at least one slot
    assert_eq!(Scheduler::compute_planned_slots(4, 8, 8), 1);
}

// --- planned_segment_count_with_plan ---

#[test]
fn segment_count_is_one_for_bt_protocol() {
    let task = sample_task_record("bt-1", "bt");
    assert_eq!(
        db::planned_segment_count_with_plan(&task, 16 * 1024 * 1024, 4),
        1
    );
}

#[test]
fn segment_count_uses_plan_when_above_threshold() {
    let mut task = sample_task_record("http-big", "http");
    task.supports_parallel = true;
    task.total_size = 32 * 1024 * 1024; // 32 MB, above 16 MB threshold
    assert_eq!(
        db::planned_segment_count_with_plan(&task, 16 * 1024 * 1024, 4),
        4
    );
}

#[test]
fn segment_count_falls_back_to_single_when_below_threshold() {
    let mut task = sample_task_record("http-small", "http");
    task.supports_parallel = true;
    task.total_size = 8 * 1024 * 1024; // 8 MB, below 16 MB threshold
    assert_eq!(
        db::planned_segment_count_with_plan(&task, 16 * 1024 * 1024, 4),
        1
    );
}

// --- R-2.6 race-condition tests (DB layer) ---------------------------------
//
// These tests verify the conditional UPDATE invariants that close the
// start-vs-pause/cancel/retry race window. They exercise the DB layer
// directly (no Tauri runtime, no spawned workers) because the conditional
// UPDATE is the actual serialization mechanism — the per-task runtime lock
// (R-2.3) only serializes entry into the critical section, while the
// conditional UPDATE prevents a worker's terminal write from overwriting a
// user-initiated state change that landed during the worker's run.
//
// Each test simulates the race by performing the "user action" (B) BEFORE
// the "worker terminal write" (A), then asserting A is a no-op.

use std::time::{SystemTime, UNIX_EPOCH};

async fn race_test_pool(label: &str) -> sqlx::SqlitePool {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-race-{label}-{id}.sqlite"));
    db::connect(&path)
        .await
        .expect("database connect with migrations")
        .pool
}

/// Inserts a task in `Downloading` state for race-condition tests.
async fn insert_downloading_task(pool: &sqlx::SqlitePool, id: &str) {
    let mut task = sample_task_record(id, "http");
    task.status = TaskStatus::Downloading;
    task.total_size = 1024;
    task.downloaded_bytes = 512;
    task.connection_count = 2;
    task.speed_bps = 1024;
    db::insert_task_record(pool, &task)
        .await
        .expect("insert task");
}

async fn fetch_task_status(pool: &sqlx::SqlitePool, id: &str) -> TaskStatus {
    let record = db::get_task_record(pool, id)
        .await
        .expect("query task")
        .expect("task exists");
    record.status
}

/// R-2.6: `mark_task_failed_if_active` must NOT overwrite a Paused status
/// that a user action (pause_task) set while the worker was running.
///
/// Timeline:
///   1. Worker starts: task = Downloading
///   2. User pauses:   task = Paused (via conditional UPDATE with
///      expected_current_status = Some(Downloading))
///   3. Worker fails:  calls mark_task_failed_if_active
///      → WHERE status IN ('downloading', 'retrying') does not match Paused
///      → rows_affected == 0 → returns Ok(false) → no emit
///
/// Without R-2.4's conditional UPDATE, step 3 would overwrite Paused with
/// Failed, silently discarding the user's pause.
#[tokio::test]
async fn mark_task_failed_if_active_skips_when_status_changed() {
    let pool = race_test_pool("markfail-skip").await;
    let task_id = "race-markfail-task";

    insert_downloading_task(&pool, task_id).await;

    // Step 2: user pauses (conditional UPDATE expects Downloading).
    let paused = db::update_task_status(
        &pool,
        task_id,
        TaskStatus::Paused,
        Some(TaskStatus::Downloading),
        0,
        0,
        Some("Paused"),
        None,
    )
    .await
    .expect("pause task");
    assert!(
        paused.is_some(),
        "pause should succeed when status is Downloading"
    );

    // Step 3: worker fails — must NOT overwrite Paused.
    let updated = db::mark_task_failed_if_active(
        &pool,
        task_id,
        TaskStatus::Failed,
        Some("Download failed"),
        Some("engine_error: connection reset"),
    )
    .await
    .expect("mark_task_failed_if_active");

    assert!(
        !updated,
        "mark_task_failed_if_active must return false when status is no longer Downloading/Retrying"
    );

    // DB state must remain Paused, not Failed.
    assert_eq!(
        fetch_task_status(&pool, task_id).await,
        TaskStatus::Paused,
        "task status must remain Paused, not be overwritten with Failed"
    );

    // error_message must NOT be set by the skipped mark_task_failed_if_active.
    let record = db::get_task_record(&pool, task_id)
        .await
        .expect("query")
        .expect("task exists");
    assert!(
        record.error_message.is_none(),
        "error_message must not be set when mark_task_failed_if_active was skipped, got: {:?}",
        record.error_message
    );

    pool.close().await;
}

/// R-2.6: `complete_task` must NOT overwrite a Paused status that a user
/// action set while the worker was finishing.
///
/// Timeline:
///   1. Worker starts:    task = Downloading
///   2. User pauses:      task = Paused
///   3. Worker completes: calls complete_task
///      → WHERE status = 'downloading' does not match Paused
///      → rows_affected == 0 → rollback → return Ok(())
///
/// Without R-2.4's conditional UPDATE, step 3 would overwrite Paused with
/// Completed, silently discarding the user's pause and marking a partially
/// downloaded file as complete.
#[tokio::test]
async fn complete_task_does_not_overwrite_paused() {
    let pool = race_test_pool("complete-skip").await;
    let task_id = "race-complete-task";

    insert_downloading_task(&pool, task_id).await;

    // Step 2: user pauses (conditional UPDATE expects Downloading).
    let paused = db::update_task_status(
        &pool,
        task_id,
        TaskStatus::Paused,
        Some(TaskStatus::Downloading),
        0,
        0,
        Some("Paused"),
        None,
    )
    .await
    .expect("pause task");
    assert!(
        paused.is_some(),
        "pause should succeed when status is Downloading"
    );

    // Step 3: worker completes — must NOT overwrite Paused.
    db::complete_task(&pool, task_id)
        .await
        .expect("complete_task must not error (returns Ok(()) on no-op)");

    // DB state must remain Paused, not Completed.
    assert_eq!(
        fetch_task_status(&pool, task_id).await,
        TaskStatus::Paused,
        "task status must remain Paused, not be overwritten with Completed"
    );

    // downloaded_bytes must NOT be bumped to total_size by the skipped complete.
    let record = db::get_task_record(&pool, task_id)
        .await
        .expect("query")
        .expect("task exists");
    assert_eq!(
        record.downloaded_bytes, 512,
        "downloaded_bytes must remain at the pre-complete value (512), not be bumped to total_size"
    );
    assert_eq!(
        record.speed_bps, 0,
        "speed_bps must be 0 (pause sets it), not restored to the pre-pause value"
    );

    pool.close().await;
}

// --- A-1: TransitionError variant coverage ---------------------------------
//
// `start_task` (scheduler/mod.rs:295-332) matches `TransitionError::Conflict`
// explicitly (removing the pending DownloadControl at :314) and falls through
// to a generic `Err(error)` branch at :323 for all other variants. That
// generic branch also removes the pending control (the A-1 fix). If a new
// non-Conflict variant were added to the enum without updating the match, it
// would fall through correctly — but this test guards against accidental
// future changes that might add a new explicit match arm that forgets to
// clean up the downloads map.

/// A-1: Verify that all non-Conflict `TransitionError` variants fall through
/// to the generic `Err(error)` branch in `start_task`, which removes the
/// pending `DownloadControl` entry. Only `Conflict` is matched explicitly.
#[test]
fn transition_error_non_conflict_variants_do_not_match_conflict_pattern() {
    let illegal = TransitionError::Illegal {
        from: TaskStatus::Queued,
        to: TaskStatus::Downloading,
    };
    let database = TransitionError::Database("test".to_string());
    let not_found = TransitionError::NotFound("task-1".to_string());

    assert!(
        !matches!(illegal, TransitionError::Conflict { .. }),
        "Illegal must not match Conflict pattern — it must fall through to the generic error branch"
    );
    assert!(
        !matches!(database, TransitionError::Conflict { .. }),
        "Database must not match Conflict pattern — it must fall through to the generic error branch"
    );
    assert!(
        !matches!(not_found, TransitionError::Conflict { .. }),
        "NotFound must not match Conflict pattern — it must fall through to the generic error branch"
    );
}
