use tauri::State;
use uuid::Uuid;

use crate::{
    db,
    models::{
        task::now_iso, HashVerificationStatus, RequestDiagnosticRecord, ScaleStateDistribution,
        SegmentStatus, Task, TaskFileRecord, TaskKind, TaskPriority, TaskRecord, TaskSegmentRecord,
        TaskStatus,
    },
    AppState,
};

use super::task_from_record_with_files;

#[cfg(debug_assertions)]
pub async fn seed_mock_data(pool: &sqlx::SqlitePool) -> Result<Vec<Task>, String> {
    db::clear_tasks(pool).await?;
    crate::events::clear_task_files_version_cache();
    let now = now_iso();
    let mocks = build_mock_tasks(&now);

    for task in &mocks {
        db::insert_task_record(pool, task).await?;
        db::insert_task_file_record(
            pool,
            &TaskFileRecord {
                id: Uuid::new_v4().to_string(),
                task_id: task.id.clone(),
                relative_path: task.file_name.clone(),
                file_name: task.file_name.clone(),
                save_dir: task.save_dir.clone(),
                temp_path: task.temp_path.clone(),
                final_path: task.final_path.clone(),
                total_size: task.total_size,
                downloaded_bytes: task.downloaded_bytes,
                selected: true,
                status: task.status,
                content_type: task.content_type.clone(),
            },
        )
        .await?;
    }

    let records = db::list_task_records(pool).await?;
    let mut tasks = Vec::with_capacity(records.len());
    for record in records {
        tasks.push(task_from_record_with_files(pool, record).await?);
    }
    Ok(tasks)
}

#[cfg(debug_assertions)]
#[tauri::command]
#[specta::specta]
pub async fn seed_mock_tasks(state: State<'_, AppState>) -> Result<Vec<Task>, String> {
    seed_mock_data(&state.pool).await
}

#[cfg(debug_assertions)]
fn build_mock_tasks(now: &str) -> Vec<TaskRecord> {
    [
        MockTaskInput {
            file_name: "ubuntu-24.04.iso",
            url: "https://releases.ubuntu.com/noble/ubuntu-24.04-desktop-amd64.iso",
            host: "releases.ubuntu.com",
            status: TaskStatus::Downloading,
            total_size: 4_200_000_000,
            downloaded_bytes: 1_680_000_000,
            connection_count: 8,
            speed_bps: 48_500_000,
            health_summary: Some("Downloading steadily".into()),
        },
        MockTaskInput {
            file_name: "node-v22.pkg",
            url: "https://nodejs.org/dist/v22.0.0/node-v22.0.0.pkg",
            host: "nodejs.org",
            status: TaskStatus::Downloading,
            total_size: 80_000_000,
            downloaded_bytes: 52_000_000,
            connection_count: 4,
            speed_bps: 12_400_000,
            health_summary: Some("Server limit detected".into()),
        },
        MockTaskInput {
            file_name: "rust-docs.pdf",
            url: "https://doc.rust-lang.org/book.pdf",
            host: "doc.rust-lang.org",
            status: TaskStatus::Paused,
            total_size: 12_000_000,
            downloaded_bytes: 4_800_000,
            connection_count: 0,
            speed_bps: 0,
            health_summary: None,
        },
        MockTaskInput {
            file_name: "game-patch.zip",
            url: "https://cdn.example.com/patches/season-12.zip",
            host: "cdn.example.com",
            status: TaskStatus::Queued,
            total_size: 2_400_000_000,
            downloaded_bytes: 0,
            connection_count: 0,
            speed_bps: 0,
            health_summary: None,
        },
        MockTaskInput {
            file_name: "dataset.tar.gz",
            url: "https://data.example.org/ml/dataset.tar.gz",
            host: "data.example.org",
            status: TaskStatus::Retrying,
            total_size: 900_000_000,
            downloaded_bytes: 120_000_000,
            connection_count: 2,
            speed_bps: 3_200_000,
            health_summary: Some("Network fluctuation, retrying".into()),
        },
        MockTaskInput {
            file_name: "driver-setup.exe",
            url: "https://vendor.example.net/drivers/setup.exe",
            host: "vendor.example.net",
            status: TaskStatus::Failed,
            total_size: 350_000_000,
            downloaded_bytes: 89_000_000,
            connection_count: 0,
            speed_bps: 0,
            health_summary: Some("Resume unavailable".into()),
        },
        MockTaskInput {
            file_name: "llm-weights.safetensors",
            url: "https://models.example.ai/weights/v3.safetensors",
            host: "models.example.ai",
            status: TaskStatus::NeedsAttention,
            total_size: 8_000_000_000,
            downloaded_bytes: 2_100_000_000,
            connection_count: 0,
            speed_bps: 0,
            health_summary: Some(
                "Remote file changed. Restart download to avoid corruption.".into(),
            ),
        },
        MockTaskInput {
            file_name: "archlinux.iso",
            url: "https://mirror.archlinux.org/iso/latest/archlinux-x86_64.iso",
            host: "mirror.archlinux.org",
            status: TaskStatus::Completed,
            total_size: 1_300_000_000,
            downloaded_bytes: 1_300_000_000,
            connection_count: 0,
            speed_bps: 0,
            health_summary: Some("Completed".into()),
        },
        MockTaskInput {
            file_name: "fonts-bundle.zip",
            url: "https://github.com/google/fonts/archive/refs/heads/main.zip",
            host: "github.com",
            status: TaskStatus::WaitingNetwork,
            total_size: 220_000_000,
            downloaded_bytes: 45_000_000,
            connection_count: 0,
            speed_bps: 0,
            health_summary: Some("Waiting for network".into()),
        },
        MockTaskInput {
            file_name: "vscode.deb",
            url: "https://code.visualstudio.com/sha/download?build=stable&os=linux-deb-x64",
            host: "code.visualstudio.com",
            status: TaskStatus::Downloading,
            total_size: 95_000_000,
            downloaded_bytes: 71_000_000,
            connection_count: 2,
            speed_bps: 8_900_000,
            health_summary: Some("Disk write slower than network".into()),
        },
    ]
    .into_iter()
    .map(|input| mock_task(input, now))
    .collect()
}

#[cfg(debug_assertions)]
struct MockTaskInput {
    file_name: &'static str,
    url: &'static str,
    host: &'static str,
    status: TaskStatus,
    total_size: i64,
    downloaded_bytes: i64,
    connection_count: i32,
    speed_bps: i64,
    health_summary: Option<String>,
}

#[cfg(debug_assertions)]
fn mock_task(input: MockTaskInput, now: &str) -> TaskRecord {
    let MockTaskInput {
        file_name,
        url,
        host,
        status,
        total_size,
        downloaded_bytes,
        connection_count,
        speed_bps,
        health_summary,
    } = input;
    let error_message = if matches!(status, TaskStatus::Failed | TaskStatus::NeedsAttention) {
        health_summary.clone()
    } else {
        None
    };

    TaskRecord {
        id: Uuid::new_v4().to_string(),
        url: url.to_string(),
        final_url: Some(url.to_string()),
        protocol: "https".to_string(),
        task_kind: crate::models::TaskKind::SingleFile,
        file_name: file_name.to_string(),
        save_dir: "~/Downloads".to_string(),
        temp_path: None,
        final_path: None,
        total_size,
        downloaded_bytes,
        status,
        etag: None,
        last_modified: None,
        content_type: None,
        supports_resume: true,
        supports_parallel: true,
        supports_multi_file: false,
        source_key: host.to_string(),
        connection_count,
        speed_bps,
        task_speed_limit_bps: None,
        priority: crate::models::TaskPriority::Normal,
        queue_position: 0,
        category_key: None,
        obey_schedule: true,
        health_summary,
        error_message,
        error_code: None,
        recovery_actions: Vec::new(),
        retry_after_at: None,
        expected_hash_sha256: None,
        actual_hash_sha256: None,
        hash_status: HashVerificationStatus::NotRequested,
        hash_error: None,
        hash_verified_at: None,
        created_at: now.to_string(),
        updated_at: now.to_string(),
        files_version: 0,
    }
}

// ---------------------------------------------------------------------------
// Parameterized scale seeder (debug-only, for performance stress testing)
// ---------------------------------------------------------------------------

const SCALE_SEGMENT_COUNT: i64 = 4;
const SCALE_BASE_SIZE: i64 = 100_000_000; // 100 MB

#[cfg(debug_assertions)]
pub async fn seed_scale_data(
    pool: &sqlx::SqlitePool,
    distribution: &ScaleStateDistribution,
    clear_before: bool,
) -> Result<u32, String> {
    if clear_before {
        db::clear_tasks(pool).await?;
        crate::events::clear_task_files_version_cache();
    }

    // When appending, offset the index by existing task count so source_key
    // stays unique across batches.
    let mut index: i64 = if clear_before {
        0
    } else {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tasks")
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?
    };
    let now = now_iso();
    let mut total = 0u32;

    for _ in 0..distribution.queued {
        let task = build_scale_task(index, TaskStatus::Queued, &now);
        insert_scale_task_with_children(pool, &task, TaskStatus::Queued).await?;
        index += 1;
        total += 1;
    }
    for _ in 0..distribution.downloading {
        let task = build_scale_task(index, TaskStatus::Downloading, &now);
        insert_scale_task_with_children(pool, &task, TaskStatus::Downloading).await?;
        index += 1;
        total += 1;
    }
    for _ in 0..distribution.completed {
        let task = build_scale_task(index, TaskStatus::Completed, &now);
        insert_scale_task_with_children(pool, &task, TaskStatus::Completed).await?;
        index += 1;
        total += 1;
    }
    for _ in 0..distribution.failed {
        let task = build_scale_task(index, TaskStatus::Failed, &now);
        insert_scale_task_with_children(pool, &task, TaskStatus::Failed).await?;
        index += 1;
        total += 1;
    }

    Ok(total)
}

#[cfg(debug_assertions)]
#[tauri::command]
#[specta::specta]
pub async fn seed_scale_tasks(
    state: State<'_, AppState>,
    distribution: ScaleStateDistribution,
    clear_before: Option<bool>,
) -> Result<u32, String> {
    seed_scale_data(&state.pool, &distribution, clear_before.unwrap_or(false)).await
}

#[cfg(debug_assertions)]
fn build_scale_task(index: i64, status: TaskStatus, now: &str) -> TaskRecord {
    // Vary size deterministically (50 MB – 500 MB) to avoid a uniform dataset.
    let total_size = SCALE_BASE_SIZE + (index % 10) * 45_000_000;
    let downloaded = match status {
        TaskStatus::Queued => 0,
        TaskStatus::Downloading => total_size / 2,
        TaskStatus::Completed => total_size,
        TaskStatus::Failed => total_size * 3 / 10,
        _ => 0,
    };
    let (connection_count, speed_bps) = match status {
        TaskStatus::Downloading => (4, 5_000_000),
        _ => (0, 0),
    };
    let error_message = if matches!(status, TaskStatus::Failed) {
        Some("Connection reset by peer".to_string())
    } else {
        None
    };
    let error_code = if matches!(status, TaskStatus::Failed) {
        Some("http_request_failed".to_string())
    } else {
        None
    };
    let health_summary = match status {
        TaskStatus::Downloading => Some("Downloading steadily".to_string()),
        TaskStatus::Failed => Some("Connection reset".to_string()),
        TaskStatus::Completed => Some("Completed".to_string()),
        _ => None,
    };

    TaskRecord {
        id: Uuid::new_v4().to_string(),
        url: format!("https://scale-{index}.example.com/file.bin"),
        final_url: Some(format!("https://scale-{index}.example.com/file.bin")),
        protocol: "https".to_string(),
        task_kind: TaskKind::SingleFile,
        file_name: format!("scale-file-{index}.bin"),
        save_dir: "~/Downloads".to_string(),
        temp_path: None,
        final_path: None,
        total_size,
        downloaded_bytes: downloaded,
        status,
        etag: None,
        last_modified: None,
        content_type: Some("application/octet-stream".to_string()),
        supports_resume: true,
        supports_parallel: true,
        supports_multi_file: false,
        source_key: format!("scale-{index}.example.com"),
        connection_count,
        speed_bps,
        task_speed_limit_bps: None,
        priority: TaskPriority::Normal,
        queue_position: index,
        category_key: None,
        obey_schedule: true,
        health_summary,
        error_message,
        error_code,
        recovery_actions: Vec::new(),
        retry_after_at: None,
        expected_hash_sha256: None,
        actual_hash_sha256: None,
        hash_status: HashVerificationStatus::NotRequested,
        hash_error: None,
        hash_verified_at: None,
        created_at: now.to_string(),
        updated_at: now.to_string(),
        files_version: 0,
    }
}

#[cfg(debug_assertions)]
async fn insert_scale_task_with_children(
    pool: &sqlx::SqlitePool,
    task: &TaskRecord,
    status: TaskStatus,
) -> Result<(), String> {
    db::insert_task_record(pool, task).await?;

    let file_id = Uuid::new_v4().to_string();
    db::insert_task_file_record(
        pool,
        &TaskFileRecord {
            id: file_id.clone(),
            task_id: task.id.clone(),
            relative_path: task.file_name.clone(),
            file_name: task.file_name.clone(),
            save_dir: task.save_dir.clone(),
            temp_path: None,
            final_path: None,
            total_size: task.total_size,
            downloaded_bytes: task.downloaded_bytes,
            selected: true,
            status: task.status,
            content_type: task.content_type.clone(),
        },
    )
    .await?;

    // Segments: Queued → 0, otherwise 4 segments with state-appropriate progress.
    if status != TaskStatus::Queued {
        let segment_size = task.total_size / SCALE_SEGMENT_COUNT;
        for i in 0..SCALE_SEGMENT_COUNT {
            let range_start = i * segment_size;
            let range_end = if i == SCALE_SEGMENT_COUNT - 1 {
                task.total_size
            } else {
                (i + 1) * segment_size
            };
            let (seg_status, downloaded_until, last_error, speed) = match status {
                TaskStatus::Completed => {
                    (SegmentStatus::Completed, range_end - range_start, None, 0)
                }
                TaskStatus::Failed if i >= 2 => (
                    SegmentStatus::Failed,
                    (range_end - range_start) / 2,
                    Some("Connection reset by peer".to_string()),
                    0,
                ),
                TaskStatus::Downloading if i >= 2 => (
                    SegmentStatus::Downloading,
                    (range_end - range_start) / 2,
                    None,
                    1_250_000,
                ),
                _ => (SegmentStatus::Completed, range_end - range_start, None, 0),
            };
            db::insert_segment_record(
                pool,
                &TaskSegmentRecord {
                    id: Uuid::new_v4().to_string(),
                    task_id: task.id.clone(),
                    file_id: Some(file_id.clone()),
                    unit_kind: "http_range".to_string(),
                    range_start,
                    range_end,
                    downloaded_until,
                    speed_bps: speed,
                    status: seg_status,
                    retry_count: if matches!(seg_status, SegmentStatus::Failed) {
                        2
                    } else {
                        0
                    },
                    last_error,
                },
            )
            .await?;
        }
    }

    // Events: 1 for Queued, 2 for others.
    db::insert_task_event(pool, &task.id, "task_created", None).await?;
    match status {
        TaskStatus::Downloading => {
            db::insert_task_event(pool, &task.id, "download_started", None).await?;
        }
        TaskStatus::Completed => {
            db::insert_task_event(pool, &task.id, "download_completed", None).await?;
        }
        TaskStatus::Failed => {
            db::insert_task_event(
                pool,
                &task.id,
                "download_failed",
                Some("{\"code\":\"http_request_failed\"}"),
            )
            .await?;
        }
        _ => {}
    }

    // Request diagnostics: Queued → 0, otherwise 2 per task.
    if status != TaskStatus::Queued {
        for i in 0..2i64 {
            let is_error = matches!(status, TaskStatus::Failed) && i == 1;
            db::insert_request_diagnostic(
                pool,
                &RequestDiagnosticRecord {
                    task_id: task.id.clone(),
                    method: "GET".to_string(),
                    url: task.url.clone(),
                    range_header: Some(format!(
                        "bytes={}-{}",
                        i * (task.total_size / 2),
                        (i + 1) * (task.total_size / 2) - 1
                    )),
                    if_range_header: None,
                    status_code: if is_error { Some(500) } else { Some(206) },
                    etag: if is_error {
                        None
                    } else {
                        Some("\"abc123\"".to_string())
                    },
                    last_modified: None,
                    content_length: Some(task.total_size / 2),
                    error_message: if is_error {
                        Some("Internal Server Error".to_string())
                    } else {
                        None
                    },
                    retry_count: if is_error { 1 } else { 0 },
                    duration_ms: if is_error { 5000 } else { 250 },
                },
            )
            .await?;
        }
    }

    Ok(())
}
