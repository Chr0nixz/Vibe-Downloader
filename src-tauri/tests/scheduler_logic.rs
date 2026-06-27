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
        true,  // enabled
        true,  // obey_schedule
        true,  // time_window_active
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
    assert_eq!(db::planned_segment_count_with_plan(&task, 16 * 1024 * 1024, 4), 1);
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
