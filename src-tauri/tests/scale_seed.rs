//! Tests for the debug-only `seed_scale_tasks` parameterized seeder.
//!
//! These tests verify that the scale seeder generates the correct number of
//! tasks per state, creates associated segments/events/request-diagnostics,
//! and respects the `clear_before` flag for append vs. replace semantics.

#![cfg(debug_assertions)]

use std::time::{SystemTime, UNIX_EPOCH};

use tauri_app_lib::{
    commands::tasks::seed_scale_data,
    db,
    models::{ScaleStateDistribution, TaskStatus},
};

async fn test_pool(label: &str) -> sqlx::SqlitePool {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-db-scale-{label}-{id}.sqlite"));
    db::connect(&path).await.expect("connect").pool
}

fn distribution(queued: i32, downloading: i32, completed: i32, failed: i32) -> ScaleStateDistribution {
    ScaleStateDistribution {
        queued,
        downloading,
        completed,
        failed,
    }
}

#[tokio::test]
async fn generates_correct_total_count() {
    let pool = test_pool("total-count").await;
    let dist = distribution(10, 20, 30, 40);
    let count = seed_scale_data(&pool, &dist, true)
        .await
        .expect("seed");
    assert_eq!(count, 100);
}

#[tokio::test]
async fn generates_correct_state_distribution() {
    let pool = test_pool("state-dist").await;
    let dist = distribution(5, 8, 12, 3);
    seed_scale_data(&pool, &dist, true)
        .await
        .expect("seed");

    let records = db::list_task_records(&pool).await.expect("list");
    let queued = records.iter().filter(|t| t.status == TaskStatus::Queued).count();
    let downloading = records.iter().filter(|t| t.status == TaskStatus::Downloading).count();
    let completed = records.iter().filter(|t| t.status == TaskStatus::Completed).count();
    let failed = records.iter().filter(|t| t.status == TaskStatus::Failed).count();

    assert_eq!(queued, 5);
    assert_eq!(downloading, 8);
    assert_eq!(completed, 12);
    assert_eq!(failed, 3);
}

#[tokio::test]
async fn clear_before_wipes_existing_tasks() {
    let pool = test_pool("clear-wipe").await;
    // First batch: 5 tasks
    seed_scale_data(&pool, &distribution(2, 1, 1, 1), true)
        .await
        .expect("first seed");
    assert_eq!(db::list_task_records(&pool).await.unwrap().len(), 5);

    // Second batch with clear_before=true: old tasks gone, only new ones remain
    seed_scale_data(&pool, &distribution(10, 0, 0, 0), true)
        .await
        .expect("second seed");
    assert_eq!(db::list_task_records(&pool).await.unwrap().len(), 10);
}

#[tokio::test]
async fn append_mode_preserves_existing_tasks() {
    let pool = test_pool("append").await;
    seed_scale_data(&pool, &distribution(3, 0, 0, 0), true)
        .await
        .expect("first seed");
    assert_eq!(db::list_task_records(&pool).await.unwrap().len(), 3);

    // Append 7 more → total 10
    seed_scale_data(&pool, &distribution(0, 4, 2, 1), false)
        .await
        .expect("append");
    let records = db::list_task_records(&pool).await.unwrap();
    assert_eq!(records.len(), 10);
    assert_eq!(
        records.iter().filter(|t| t.status == TaskStatus::Queued).count(),
        3
    );
    assert_eq!(
        records.iter().filter(|t| t.status == TaskStatus::Downloading).count(),
        4
    );
}

#[tokio::test]
async fn generates_segments_for_non_queued_tasks() {
    let pool = test_pool("segments").await;
    seed_scale_data(&pool, &distribution(1, 1, 1, 1), true)
        .await
        .expect("seed");

    let records = db::list_task_records(&pool).await.unwrap();
    for record in &records {
        let segments = db::list_segment_records(&pool, &record.id).await.unwrap();
        match record.status {
            TaskStatus::Queued => {
                // Queued tasks have no segments (pending, not yet planned)
                assert!(
                    segments.is_empty(),
                    "Queued task {} should have 0 segments, got {}",
                    record.id,
                    segments.len()
                );
            }
            _ => {
                // Non-queued tasks get 4 segments each
                assert_eq!(
                    segments.len(),
                    4,
                    "Task {} ({:?}) should have 4 segments",
                    record.id,
                    record.status
                );
            }
        }
    }
}

#[tokio::test]
async fn generates_events_for_all_tasks() {
    let pool = test_pool("events").await;
    seed_scale_data(&pool, &distribution(1, 1, 1, 1), true)
        .await
        .expect("seed");

    let records = db::list_task_records(&pool).await.unwrap();
    for record in &records {
        let events = db::list_task_events_page(&pool, &record.id, None, 500)
            .await
            .unwrap();
        match record.status {
            TaskStatus::Queued => {
                // Queued: 1 event (task_created)
                assert_eq!(events.len(), 1, "Queued task should have 1 event");
            }
            _ => {
                // Others: 2 events (task_created + state-specific)
                assert_eq!(events.len(), 2, "Task {:?} should have 2 events", record.status);
            }
        }
    }
}

#[tokio::test]
async fn generates_request_diagnostics_for_non_queued_tasks() {
    let pool = test_pool("requests").await;
    seed_scale_data(&pool, &distribution(1, 1, 1, 1), true)
        .await
        .expect("seed");

    let records = db::list_task_records(&pool).await.unwrap();
    for record in &records {
        let requests = db::list_request_diagnostics_page(&pool, &record.id, None, 500)
            .await
            .unwrap();
        match record.status {
            TaskStatus::Queued => {
                assert!(
                    requests.is_empty(),
                    "Queued task should have 0 request diagnostics"
                );
            }
            _ => {
                // 2 request diagnostics per non-queued task
                assert_eq!(
                    requests.len(),
                    2,
                    "Task {:?} should have 2 request diagnostics",
                    record.status
                );
            }
        }
    }
}

#[tokio::test]
async fn failed_tasks_have_error_metadata() {
    let pool = test_pool("errors").await;
    seed_scale_data(&pool, &distribution(0, 0, 0, 5), true)
        .await
        .expect("seed");

    let records = db::list_task_records(&pool).await.unwrap();
    assert_eq!(records.len(), 5);
    for record in &records {
        assert_eq!(record.status, TaskStatus::Failed);
        assert!(record.error_message.is_some(), "Failed task should have error_message");
        assert!(record.error_code.is_some(), "Failed task should have error_code");
        assert_eq!(record.error_code.as_deref(), Some("http_request_failed"));
    }
}
