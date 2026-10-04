mod common;

use tauri_app_lib::{
    db,
    models::{AppErrorPayload, TaskStatus},
};

#[tokio::test]
async fn automatic_retry_persists_budget_and_respects_state_changes() {
    let (_db, pool) = common::test_pool("task-retry").await;
    let paths = common::TestPaths::new("task-retry");
    let task = common::download_task(
        "task-auto-retry",
        "http://127.0.0.1/file.bin".to_string(),
        "http",
        "file.bin",
        10,
        &paths,
        false,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");
    assert!(db::mark_task_failed_if_active(
        &pool,
        &task.id,
        TaskStatus::Failed,
        Some("Failed"),
        Some("temporary upstream failure"),
    )
    .await
    .expect("mark coordinator failure"));
    let error = AppErrorPayload::new(
        "server_error",
        "temporary upstream failure",
        true,
        vec!["retry_later"],
    )
    .command_error();

    let scheduled = db::schedule_auto_retry(
        &pool,
        &task.id,
        1,
        "2099-01-01T00:00:00Z",
        "server_error",
        &error,
    )
    .await
    .expect("schedule retry");
    assert_eq!(scheduled, db::AutoRetryOutcome::Scheduled { attempt: 1 });
    let queued = db::get_task_record(&pool, &task.id)
        .await
        .expect("load queued")
        .expect("task exists");
    assert_eq!(queued.status, TaskStatus::Queued);
    assert_eq!(
        queued.retry_after_at.as_deref(),
        Some("2099-01-01T00:00:00Z")
    );
    assert_eq!(db::auto_retry_attempt(&pool, &task.id).await.unwrap(), 1);

    db::update_task_status(
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
    .expect("pause task");
    assert_eq!(
        db::schedule_auto_retry(
            &pool,
            &task.id,
            2,
            "2099-01-01T00:00:00Z",
            "server_error",
            &error,
        )
        .await
        .expect("state-changed retry"),
        db::AutoRetryOutcome::StateChanged
    );
    let paused = db::get_task_record(&pool, &task.id)
        .await
        .expect("load paused")
        .expect("task exists");
    assert_eq!(paused.status, TaskStatus::Paused);
    assert!(
        paused.retry_after_at.is_none(),
        "pause clears the wake deadline"
    );
}

#[tokio::test]
async fn automatic_retry_exhaustion_is_terminal() {
    let (_db, pool) = common::test_pool("task-retry-exhausted").await;
    let paths = common::TestPaths::new("task-retry-exhausted");
    let task = common::download_task(
        "task-auto-retry-exhausted",
        "http://127.0.0.1/file.bin".to_string(),
        "http",
        "file.bin",
        10,
        &paths,
        false,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");
    let error = AppErrorPayload::new("timeout", "timed out", true, vec!["retry"]).command_error();
    let result = db::schedule_auto_retry(
        &pool,
        &task.id,
        11,
        "2099-01-01T00:00:00Z",
        "timeout",
        &error,
    )
    .await
    .expect("exhaust retry");
    assert_eq!(result, db::AutoRetryOutcome::Exhausted { attempt: 11 });
    let failed = db::get_task_record(&pool, &task.id)
        .await
        .expect("load failed")
        .expect("task exists");
    assert_eq!(failed.status, TaskStatus::Failed);
    assert!(failed.retry_after_at.is_none());
    assert_eq!(db::auto_retry_attempt(&pool, &task.id).await.unwrap(), 11);
    let reason: String =
        sqlx::query_scalar("SELECT reason FROM task_auto_retry_state WHERE task_id = ?")
            .bind(&task.id)
            .fetch_one(&pool)
            .await
            .expect("persist final retry reason");
    assert_eq!(reason, "timeout");
}
