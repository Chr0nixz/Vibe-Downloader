//! ARC-38: startup sweep for orphaned HLS/DASH staging directories.
//!
//! Staging (`{save_dir}/.vibe-staging/{task_id}`) holds near-final-size
//! segment data. The sweep must remove it when the owning task is gone (the
//! delete-path leak) or Completed (post-publish leftover), and must keep it
//! for resumable tasks — retry and resume semantics depend on staging exactly
//! like on HTTP temp files.

use std::{
    collections::HashMap,
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use tauri_app_lib::{commands::task_file_planning, db, models::TaskStatus};

mod common;

fn unique_base(label: &str) -> std::path::PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!("vibe-arc38-sweep-{label}-{id}"))
}

fn make_staging_dir(save_dir: &std::path::Path, task_id: &str) -> std::path::PathBuf {
    let dir = task_file_planning::task_staging_dir(save_dir, task_id);
    fs::create_dir_all(&dir).expect("create staging dir");
    // A segment-sized payload so the test would notice real disk usage.
    fs::write(dir.join("segment-0.ts"), vec![0_u8; 4096]).expect("write segment");
    dir
}

#[tokio::test]
async fn sweep_removes_orphan_and_completed_staging_keeps_resumable() {
    let base = unique_base("mixed");
    fs::create_dir_all(&base).expect("create base");
    let save_dir = base.join("downloads");
    fs::create_dir_all(&save_dir).expect("create save dir");

    let pool = common::test_pool("arc38-sweep").await;

    let now = chrono::Utc::now().to_rfc3339();
    for (id, status) in [
        ("task-completed", TaskStatus::Completed),
        ("task-paused", TaskStatus::Paused),
        ("task-attention", TaskStatus::NeedsAttention),
    ] {
        let record = common::download_task(
            id,
            format!("https://example.com/{id}.bin"),
            "https",
            &format!("{id}.bin"),
            0,
            // download_task derives save_dir/temp/final from paths; give every
            // task the same save dir so one .vibe-staging root is shared.
            &common::TestPaths {
                temp: base.join("unused").join(format!("{id}.tmp")),
                final_path: save_dir.join(format!("{id}.bin")),
            },
            true,
        );
        // download_task defaults status to Downloading; override per case.
        let record = tauri_app_lib::models::TaskRecord {
            status: status.clone(),
            updated_at: now.clone(),
            ..record
        };
        db::insert_task_record(&pool, &record)
            .await
            .expect("insert task");
    }

    make_staging_dir(&save_dir, "task-completed"); // Completed → remove
    make_staging_dir(&save_dir, "task-paused"); // Paused (resumable) → keep
    make_staging_dir(&save_dir, "task-attention"); // NeedsAttention (resumable) → keep
    make_staging_dir(&save_dir, "task-deleted"); // No task row → remove
    make_staging_dir(&save_dir, "never-existed"); // No task row → remove

    let removed = task_file_planning::sweep_orphan_staging_dirs(&pool)
        .await
        .expect("sweep");

    assert_eq!(
        removed, 3,
        "sweep must remove exactly the orphan and completed staging dirs"
    );
    // An empty task id resolves to the `.vibe-staging` root itself.
    let staging_root = task_file_planning::task_staging_dir(&save_dir, "");
    let remaining: HashMap<String, ()> = fs::read_dir(&staging_root)
        .expect("read staging root")
        .filter_map(|entry| {
            entry
                .ok()
                .map(|e| e.file_name().to_string_lossy().to_string())
        })
        .map(|name| (name, ()))
        .collect();
    assert!(
        remaining.contains_key("task-paused"),
        "resumable task staging must survive the sweep"
    );
    assert!(
        remaining.contains_key("task-attention"),
        "needs-attention task staging must survive the sweep"
    );
    assert!(
        !remaining.contains_key("task-completed"),
        "completed task staging must be swept"
    );
    assert!(
        !remaining.contains_key("task-deleted") && !remaining.contains_key("never-existed"),
        "orphaned staging must be swept"
    );

    let _ = fs::remove_dir_all(&base);
    pool.close().await;
}

#[tokio::test]
async fn sweep_tolerates_save_dirs_without_staging_root() {
    let base = unique_base("no-root");
    fs::create_dir_all(&base).expect("create base");
    let save_dir = base.join("plain");
    fs::create_dir_all(&save_dir).expect("create save dir");

    let pool = common::test_pool("arc38-sweep-no-root").await;
    let record = common::download_task(
        "plain-task",
        "https://example.com/plain.bin".to_string(),
        "https",
        "plain.bin",
        0,
        &common::TestPaths {
            temp: base.join("plain.tmp"),
            final_path: save_dir.join("plain.bin"),
        },
        true,
    );
    db::insert_task_record(&pool, &record)
        .await
        .expect("insert");

    let removed = task_file_planning::sweep_orphan_staging_dirs(&pool)
        .await
        .expect("sweep must not fail when a save dir has no .vibe-staging root");
    assert_eq!(removed, 0);

    let _ = fs::remove_dir_all(&base);
    pool.close().await;
}
