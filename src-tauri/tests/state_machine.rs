use std::time::{SystemTime, UNIX_EPOCH};

use tauri_app_lib::db;
use tauri_app_lib::models::task::now_iso;
use tauri_app_lib::models::{
    HashVerificationStatus, TaskKind, TaskPriority, TaskRecord, TaskStatus,
};

async fn test_pool(label: &str) -> sqlx::SqlitePool {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-state-{label}-{id}.sqlite"));
    db::connect(&path)
        .await
        .expect("database connect with migrations")
        .pool
}

fn sample_task_record(id: &str, status: TaskStatus) -> TaskRecord {
    let now = now_iso();
    TaskRecord {
        id: id.to_string(),
        url: format!("https://example.com/{id}"),
        final_url: None,
        protocol: "https".to_string(),
        task_kind: TaskKind::SingleFile,
        file_name: format!("{id}.bin"),
        save_dir: std::env::temp_dir().to_string_lossy().to_string(),
        temp_path: None,
        final_path: None,
        total_size: 0,
        downloaded_bytes: 0,
        status,
        etag: None,
        last_modified: None,
        content_type: None,
        supports_resume: true,
        supports_parallel: true,
        supports_multi_file: false,
        source_key: format!("https://example.com/{id}"),
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

#[test]
fn test_legal_transitions() {
    use TaskStatus::*;
    // Queued → various
    assert!(Queued.can_transition_to(Downloading));
    assert!(Queued.can_transition_to(Paused));
    assert!(Queued.can_transition_to(Failed));
    assert!(Queued.can_transition_to(NeedsAttention));
    assert!(Queued.can_transition_to(WaitingNetwork));

    // Downloading → various
    assert!(Downloading.can_transition_to(Paused));
    assert!(Downloading.can_transition_to(Completed));
    assert!(Downloading.can_transition_to(Failed));
    assert!(Downloading.can_transition_to(Retrying));
    assert!(Downloading.can_transition_to(NeedsAttention));
    assert!(Downloading.can_transition_to(WaitingNetwork));

    // Paused → various
    assert!(Paused.can_transition_to(Queued));
    assert!(Paused.can_transition_to(Downloading));
    assert!(Paused.can_transition_to(Failed));

    // Retrying → various
    assert!(Retrying.can_transition_to(Downloading));
    assert!(Retrying.can_transition_to(Paused));
    assert!(Retrying.can_transition_to(Failed));
    assert!(Retrying.can_transition_to(NeedsAttention));

    // WaitingNetwork → various
    assert!(WaitingNetwork.can_transition_to(Queued));
    assert!(WaitingNetwork.can_transition_to(Downloading));
    assert!(WaitingNetwork.can_transition_to(Failed));

    // NeedsAttention → various
    assert!(NeedsAttention.can_transition_to(Queued));
    assert!(NeedsAttention.can_transition_to(Failed));

    // Failed → various
    assert!(Failed.can_transition_to(Queued));
    assert!(Failed.can_transition_to(Retrying));
    assert!(Failed.can_transition_to(NeedsAttention));
}

#[test]
fn test_illegal_transitions() {
    use TaskStatus::*;
    // Completed is a terminal state — no transitions out
    assert!(!Completed.can_transition_to(Downloading));
    assert!(!Completed.can_transition_to(Queued));
    assert!(!Completed.can_transition_to(Failed));
    assert!(!Completed.can_transition_to(Paused));

    // Can't jump to Completed from non-downloading states
    assert!(!Queued.can_transition_to(Completed));
    assert!(!Paused.can_transition_to(Completed));
    assert!(!Failed.can_transition_to(Completed));
    assert!(!NeedsAttention.can_transition_to(Completed));

    // Can't go back to Queued from Downloading (must pause first)
    assert!(!Downloading.can_transition_to(Queued));

    // Can't go to Retrying from non-downloading/non-failed states
    assert!(!Queued.can_transition_to(Retrying));
    assert!(!Paused.can_transition_to(Retrying));

    // Same-status transitions are not "transitions" (progress updates bypass this)
    assert!(!Queued.can_transition_to(Queued));
    assert!(!Downloading.can_transition_to(Downloading));
    assert!(!Completed.can_transition_to(Completed));
}

#[test]
fn test_completed_is_terminal() {
    use TaskStatus::*;
    for target in [
        Queued,
        Downloading,
        Paused,
        Failed,
        Retrying,
        WaitingNetwork,
        NeedsAttention,
    ] {
        assert!(
            !Completed.can_transition_to(target),
            "Completed should not transition to {:?}",
            target
        );
    }
}

/// R-1.5: Conditional update returns None when the status changed concurrently.
///
/// Simulates the race that R-1 protects against: path A reads status=Queued and
/// successfully transitions to Downloading. Path B, holding a stale view of
/// status=Queued, attempts to transition to Paused. The conditional WHERE
/// clause (`status = 'queued'`) must not match (actual is now 'downloading'),
/// so `update_task_status` returns `Ok(None)` — the caller surfaces this as
/// `TransitionError::Conflict` in `transition_task`.
#[tokio::test]
async fn conditional_update_returns_none_when_status_changed_concurrently() {
    let pool = test_pool("conflict").await;
    let task = sample_task_record("task-conflict", TaskStatus::Queued);
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");

    // Path A: expected=Queued → succeeds, status becomes Downloading.
    let updated = db::update_task_status(
        &pool,
        &task.id,
        TaskStatus::Downloading,
        Some(TaskStatus::Queued),
        0,
        1,
        Some("Downloading"),
        None,
    )
    .await
    .expect("path A update");
    assert!(
        updated.is_some(),
        "conditional update with matching expected status should succeed"
    );
    assert_eq!(updated.unwrap().status, TaskStatus::Downloading);

    // Path B: expected=Queued → must return None (status is now Downloading).
    let conflict = db::update_task_status(
        &pool,
        &task.id,
        TaskStatus::Paused,
        Some(TaskStatus::Queued),
        0,
        0,
        Some("Paused"),
        None,
    )
    .await
    .expect("path B update should not error");
    assert!(
        conflict.is_none(),
        "conditional update with stale expected status must return None (Conflict), got {conflict:?}"
    );

    // DB status must remain Downloading (path B did not overwrite).
    let actual = db::get_task_record(&pool, &task.id)
        .await
        .expect("get task")
        .expect("task exists");
    assert_eq!(
        actual.status,
        TaskStatus::Downloading,
        "concurrent path must not overwrite the actual status"
    );

    pool.close().await;
}

/// R-1.5: Conditional update does not overwrite a terminal state.
///
/// A task reaches Completed (terminal). A concurrent path holding a stale view
/// of status=Downloading attempts to mark it Failed. The conditional WHERE
/// clause (`status = 'downloading'`) must not match (actual is 'completed'),
/// so the update returns `Ok(None)` and the terminal state is preserved.
#[tokio::test]
async fn conditional_update_does_not_overwrite_terminal_state() {
    let pool = test_pool("terminal").await;
    let task = sample_task_record("task-terminal", TaskStatus::Downloading);
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");

    // Worker completes the task: expected=Downloading → Completed.
    let completed = db::update_task_status(
        &pool,
        &task.id,
        TaskStatus::Completed,
        Some(TaskStatus::Downloading),
        0,
        0,
        Some("Completed"),
        None,
    )
    .await
    .expect("complete update");
    assert!(completed.is_some(), "should complete successfully");
    assert_eq!(completed.unwrap().status, TaskStatus::Completed);

    // Concurrent failure path: expected=Downloading → must return None.
    let conflict = db::update_task_status(
        &pool,
        &task.id,
        TaskStatus::Failed,
        Some(TaskStatus::Downloading),
        0,
        0,
        Some("Failed"),
        Some("late failure"),
    )
    .await
    .expect("failure update should not error");
    assert!(
        conflict.is_none(),
        "conditional update must not overwrite terminal Completed state, got {conflict:?}"
    );

    // DB status must remain Completed.
    let actual = db::get_task_record(&pool, &task.id)
        .await
        .expect("get task")
        .expect("task exists");
    assert_eq!(
        actual.status,
        TaskStatus::Completed,
        "terminal Completed state must not be overwritten by stale failure path"
    );

    pool.close().await;
}
