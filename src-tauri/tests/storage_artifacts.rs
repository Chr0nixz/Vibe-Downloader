//! Storage & Cleanup Center integration tests: artifact scan classification,
//! the generalized startup sweep, and the per-task auxiliary artifact
//! enumeration shared by delete/restart/abandon-resume.

use std::{
    fs,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tauri_app_lib::{
    db,
    download::artifacts::{self, Ownership},
    models::{TaskRecord, TaskStatus},
};

mod common;

fn unique_base(label: &str) -> std::path::PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!("vibe-storage-{label}-{id}"))
}

const TEST_TASK_ID: &str = "0b6fd7e0-2f7a-4d3f-9a1b-2c3d4e5f6a7b";

fn temp_file_path(save_dir: &Path, stem: &str, task_id: &str) -> std::path::PathBuf {
    save_dir.join(format!("{stem}.{task_id}.vibe-downloading"))
}

fn write_file(path: &Path, size: usize) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, vec![0_u8; size]).expect("write file");
}

/// Insert a task whose status/paths differ from the default helper shape.
async fn seeded_task(pool: &sqlx::SqlitePool, record: TaskRecord) {
    db::insert_task_record(pool, &record)
        .await
        .expect("insert task");
}

#[tokio::test]
async fn scan_and_sweep_classify_reclaimable_and_resumable_artifacts() {
    let base = unique_base("mixed");
    let save_dir = base.join("downloads");
    fs::create_dir_all(&save_dir).expect("create save dir");
    let pool = common::test_pool("storage-mixed").await;

    let make_task = |id: &str, status: TaskStatus, stem: &str, temp: Option<String>| {
        let final_path = save_dir.join(format!("{stem}.bin"));
        let mut record = common::download_task(
            id,
            format!("https://example.com/{stem}.bin"),
            "https",
            &format!("{stem}.bin"),
            0,
            &common::TestPaths {
                temp: temp
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| temp_file_path(&save_dir, stem, id)),
                final_path: final_path.clone(),
            },
            true,
        );
        record.status = status;
        record
    };

    let now = chrono::Utc::now().to_rfc3339();
    let mut paused = make_task("task-paused", TaskStatus::Paused, "paused", None);
    paused.updated_at = now.clone();
    let mut downloading = make_task("task-active", TaskStatus::Downloading, "active", None);
    downloading.updated_at = now.clone();
    let mut completed = make_task("task-done", TaskStatus::Completed, "done", None);
    completed.temp_path = Some(
        // A completed task whose temp survived as a legacy-name straggler.
        save_dir
            .join("done.bin.vibe-downloading")
            .to_string_lossy()
            .to_string(),
    );
    completed.updated_at = now;
    seeded_task(&pool, paused).await;
    seeded_task(&pool, downloading).await;
    seeded_task(&pool, completed).await;

    // On-disk artifacts.
    write_file(&temp_file_path(&save_dir, "paused", "task-paused"), 4096);
    write_file(&temp_file_path(&save_dir, "active", "task-active"), 4096);
    write_file(&save_dir.join("done.bin.vibe-downloading"), 4096);
    let orphan_temp = temp_file_path(&save_dir, "orphan", TEST_TASK_ID);
    write_file(&orphan_temp, 4096);
    // Cross-volume publish leftovers always carry a UUID token (`publish_token`
    // emits the embedded task UUID or a fresh v4).
    let staging_leftover = save_dir.join(format!("publish.bin.{TEST_TASK_ID}.staging"));
    write_file(&staging_leftover, 4096);
    // User files that merely look like artifacts must survive both scan and
    // sweep (the classification grammar is deliberately strict).
    let decoy_part = save_dir.join("notes.part-1");
    let decoy_staging = save_dir.join("foo.bar.staging");
    write_file(&decoy_part, 4096);
    write_file(&decoy_staging, 4096);
    write_file(&save_dir.join("final-user-file.bin"), 8192);
    // Orphan staging dir (task row gone).
    let orphan_staging = artifacts::task_staging_dir(&save_dir, "task-gone");
    fs::create_dir_all(&orphan_staging).expect("staging dir");
    fs::write(orphan_staging.join("segment-0.ts"), vec![0_u8; 4096]).expect("segment");

    let refs = db::list_artifact_task_refs(&pool).await.expect("refs");
    let scan = artifacts::scan_save_dir(&save_dir, &refs).await;

    let ownership_of = |name_suffix: &str| {
        scan.entries
            .iter()
            .find(|entry| entry.file_name.ends_with(name_suffix))
            .map(|entry| entry.ownership.clone())
    };
    assert_eq!(
        ownership_of(&format!("{TEST_TASK_ID}.vibe-downloading")),
        Some(Ownership::Reclaimable {
            owner_task_id: None
        }),
        "orphan temp with no task row is reclaimable"
    );
    assert_eq!(
        ownership_of("task-paused.vibe-downloading"),
        Some(Ownership::Keep {
            owner_task_id: "task-paused".to_string(),
        }),
        "paused task temp is kept for resume"
    );
    assert_eq!(
        ownership_of("task-active.vibe-downloading"),
        Some(Ownership::Keep {
            owner_task_id: "task-active".to_string(),
        }),
        "downloading task temp is kept"
    );
    assert_eq!(
        ownership_of("done.bin.vibe-downloading"),
        Some(Ownership::Reclaimable {
            owner_task_id: Some("task-done".to_string()),
        }),
        "completed task legacy temp is reclaimable"
    );
    // `.vibe-staging/{task_id}` children are reported as staging entries.
    assert_eq!(
        scan.entries
            .iter()
            .find(|entry| entry.file_name == "task-gone")
            .map(|entry| entry.ownership.clone()),
        Some(Ownership::Reclaimable {
            owner_task_id: None
        }),
    );
    // User files never enter the inventory — including decoys that merely
    // look like artifacts (`*.part-N`, `*.staging` without a UUID token).
    assert!(!scan
        .entries
        .iter()
        .any(|entry| entry.file_name == "final-user-file.bin"
            || entry.file_name == "notes.part-1"
            || entry.file_name == "foo.bar.staging"),);
    assert!(!scan.truncated, "small dir must not be truncated");

    let summary = artifacts::sweep_orphan_artifacts(&pool, &[], artifacts::SweepOptions::default())
        .await
        .expect("sweep");
    assert_eq!(
        summary.removed, 4,
        "sweep removes orphan temp + publish staging + orphan staging dir + completed legacy temp"
    );
    assert_eq!(summary.failed, 0);
    assert!(
        !orphan_temp.exists() && !staging_leftover.exists() && !orphan_staging.exists(),
        "reclaimable artifacts are gone"
    );
    assert!(
        decoy_part.exists() && decoy_staging.exists(),
        "lookalike user files survive the sweep"
    );
    assert!(
        !save_dir.join("done.bin.vibe-downloading").exists(),
        "completed task legacy temp is swept"
    );
    assert!(
        temp_file_path(&save_dir, "paused", "task-paused").exists()
            && temp_file_path(&save_dir, "active", "task-active").exists(),
        "resumable temps survive the sweep"
    );

    let _ = fs::remove_dir_all(&base);
    pool.close().await;
}

#[cfg(windows)]
#[tokio::test]
async fn sweep_reports_per_entry_failure_on_locked_file() {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_SHARE_READ: u32 = 0x0000_0001;

    let base = unique_base("locked");
    let save_dir = base.join("downloads");
    fs::create_dir_all(&save_dir).expect("create save dir");
    let pool = common::test_pool("storage-locked").await;

    // Orphan artifacts, no task row at all. A handle opened without
    // FILE_SHARE_DELETE makes DeleteFile fail with a sharing violation — a
    // deterministic per-entry failure on Windows (e.g. antivirus scanners).
    let locked_temp = temp_file_path(&save_dir, "locked", TEST_TASK_ID);
    write_file(&locked_temp, 4096);
    let _handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&locked_temp)
        .expect("open without share-delete");
    let free_temp = temp_file_path(&save_dir, "free", TEST_TASK_ID);
    write_file(&free_temp, 4096);

    // The pool has no task rows, so the save dir must be passed explicitly.
    let summary = artifacts::sweep_orphan_artifacts(
        &pool,
        &[save_dir.to_string_lossy().to_string()],
        artifacts::SweepOptions::default(),
    )
    .await
    .expect("sweep must not abort on a locked entry");
    assert_eq!(summary.failed, 1, "the locked file is reported as failed");
    assert_eq!(summary.removed, 1, "the unlocked file is still removed");
    assert!(locked_temp.exists(), "locked file survives");
    assert!(!free_temp.exists(), "unlocked file is gone");

    let _ = fs::remove_dir_all(&base);
    pool.close().await;
}

#[tokio::test]
async fn sweep_covers_extra_save_dirs_without_tasks() {
    let base = unique_base("extra");
    let save_dir = base.join("empty-default");
    fs::create_dir_all(&save_dir).expect("create save dir");
    let pool = common::test_pool("storage-extra").await;

    let orphan_temp = temp_file_path(&save_dir, "orphan", TEST_TASK_ID);
    write_file(&orphan_temp, 2048);

    let summary = artifacts::sweep_orphan_artifacts(
        &pool,
        &[save_dir.to_string_lossy().to_string()],
        artifacts::SweepOptions::default(),
    )
    .await
    .expect("sweep");
    assert_eq!(summary.removed, 1, "extra dir orphan temp is swept");
    assert!(!orphan_temp.exists());

    let _ = fs::remove_dir_all(&base);
    pool.close().await;
}

#[tokio::test]
async fn stale_dht_states_are_reported_and_swept() {
    let stale_path = std::env::temp_dir().join(format!(
        "vibe-dht-{:016x}.json",
        // `as u64` already truncates the u128 nanos to the low 64 bits.
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos() as u64
    ));
    fs::write(&stale_path, "{}").expect("write dht state");
    // Backdate the file past the stale threshold. The times must be set
    // through a writable handle (Windows requires FILE_WRITE_ATTRIBUTES).
    let stale_time = UNIX_EPOCH
        + (SystemTime::now().duration_since(UNIX_EPOCH).expect("time")
            - Duration::from_secs(30 * 24 * 3600));
    let handle = fs::File::options()
        .write(true)
        .open(&stale_path)
        .expect("open dht file for times");
    handle.set_modified(stale_time).expect("backdate dht file");
    drop(handle);

    let stale_entries = artifacts::scan_stale_dht_states().await;
    assert!(
        stale_entries.iter().any(|entry| entry.path == stale_path),
        "the backdated dht file is reported as stale"
    );

    let pool = common::test_pool("storage-dht").await;
    let summary = artifacts::sweep_orphan_artifacts(
        &pool,
        &[],
        artifacts::SweepOptions { include_dht: true },
    )
    .await
    .expect("sweep");
    assert!(
        summary.removed >= 1,
        "at least the backdated dht file is removed"
    );
    assert!(!stale_path.exists(), "stale dht file is gone");
    pool.close().await;
}

#[tokio::test]
async fn auxiliary_artifacts_cover_staging_dirs_and_metalink_parts() {
    let base = unique_base("aux");
    let save_dir = base.join("downloads");
    fs::create_dir_all(&save_dir).expect("create save dir");

    // Metalink task with part siblings on disk.
    let metalink_temp = temp_file_path(&save_dir, "metalink", TEST_TASK_ID);
    write_file(&metalink_temp, 1024);
    let part0 = std::path::PathBuf::from(format!("{}.part-0", metalink_temp.display()));
    let part1 = std::path::PathBuf::from(format!("{}.part-1", metalink_temp.display()));
    write_file(&part0, 512);
    write_file(&part1, 512);
    // TestPaths::new-owned directories are removed on Drop, so the value must
    // outlive every on-disk assertion below (an inline temporary would wipe
    // the tree before task_auxiliary_artifacts runs).
    let metalink_paths = common::TestPaths {
        temp: metalink_temp.clone(),
        final_path: save_dir.join("metalink.iso"),
    };
    let metalink_task = common::download_task(
        "task-ml",
        "https://example.com/metalink.iso".to_string(),
        "metalink",
        "metalink.iso",
        0,
        &metalink_paths,
        true,
    );
    let aux = artifacts::task_auxiliary_artifacts(&metalink_task, &[]).await;
    assert!(
        aux.contains(&part0) && aux.contains(&part1),
        "metalink parts are enumerated, got {aux:?}"
    );

    // HLS task: the recorded temp IS the staging dir, still listed once.
    let hls_paths = common::TestPaths {
        temp: artifacts::task_staging_dir(&save_dir, "task-hls"),
        final_path: save_dir.join("video.mp4"),
    };
    let hls_task = common::download_task(
        "task-hls",
        "https://example.com/video.m3u8".to_string(),
        "hls",
        "video.mp4",
        0,
        &hls_paths,
        false,
    );
    let aux = artifacts::task_auxiliary_artifacts(&hls_task, &[]).await;
    assert_eq!(
        aux,
        vec![artifacts::task_staging_dir(&save_dir, "task-hls")],
        "hls staging dir is enumerated"
    );

    // Plain HTTP task: no auxiliary artifacts.
    let http_paths = common::TestPaths {
        temp: temp_file_path(&save_dir, "file", "task-http"),
        final_path: save_dir.join("file.bin"),
    };
    let http_task = common::download_task(
        "task-http",
        "https://example.com/file.bin".to_string(),
        "https",
        "file.bin",
        0,
        &http_paths,
        true,
    );
    let aux = artifacts::task_auxiliary_artifacts(&http_task, &[]).await;
    assert!(aux.is_empty(), "http task has no auxiliary artifacts");

    let _ = fs::remove_dir_all(&base);
}
