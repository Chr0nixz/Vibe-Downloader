use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use serde::Deserialize;
use specta::Type;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::{
    db,
    download::HttpEngine,
    events::{emit_queue_changed, emit_task_progress},
    models::{task::now_iso, ProbeTaskPayload, Task, TaskRecord, TaskSegment, TaskStatus},
    platform,
    AppState, DownloadControl,
};

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskInput {
    pub url: String,
    pub save_dir: Option<String>,
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProbeTaskInput {
    pub url: String,
}

#[tauri::command]
#[specta::specta]
pub async fn list_tasks(state: State<'_, AppState>) -> Result<Vec<Task>, String> {
    db::list_task_records(&state.pool)
        .await
        .map(|records| records.into_iter().map(Task::from).collect())
}

#[tauri::command]
#[specta::specta]
pub async fn get_task(state: State<'_, AppState>, id: String) -> Result<Option<Task>, String> {
    db::get_task_record(&state.pool, &id)
        .await
        .map(|record| record.map(Task::from))
}

#[tauri::command]
#[specta::specta]
pub async fn list_task_segments(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<Vec<TaskSegment>, String> {
    db::list_segment_records(&state.pool, &task_id)
        .await
        .map(|records| records.into_iter().map(TaskSegment::from).collect())
}

#[tauri::command]
#[specta::specta]
pub async fn probe_task(input: ProbeTaskInput) -> Result<ProbeTaskPayload, String> {
    let url = input.url.trim();
    if url.is_empty() {
        return Err("Enter a download URL.".to_string());
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("Only HTTP and HTTPS downloads are supported in this milestone.".to_string());
    }

    let probe = HttpEngine::new()?.probe(url).await?;
    Ok(ProbeTaskPayload {
        final_url: probe.final_url,
        file_name: probe.file_name,
        total_size: probe.total_size.to_string(),
        supports_range: probe.supports_range,
        source_host: probe.source_host,
        content_type: probe.content_type,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn create_task(
    app: AppHandle,
    state: State<'_, AppState>,
    input: CreateTaskInput,
) -> Result<Task, String> {
    let url = input.url.trim();
    if url.is_empty() {
        return Err("Enter a download URL.".to_string());
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("Only HTTP and HTTPS downloads are supported in this milestone.".to_string());
    }

    let engine = HttpEngine::new()?;
    let probe = engine.probe(url).await?;
    let save_dir = match input.save_dir.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => app
            .path()
            .download_dir()
            .map_err(|e| format!("Failed to resolve the Downloads folder: {e}"))?,
    };
    std::fs::create_dir_all(&save_dir)
        .map_err(|e| format!("Could not create the download directory: {e}"))?;

    let requested_file_name = input
        .file_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&probe.file_name);
    let final_path = unique_final_path(&save_dir, requested_file_name);
    let file_name = final_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(requested_file_name)
        .to_string();
    let temp_path = PathBuf::from(format!("{}.vibe-downloading", final_path.display()));
    let now = now_iso();

    let record = TaskRecord {
        id: Uuid::new_v4().to_string(),
        url: url.to_string(),
        final_url: Some(probe.final_url),
        file_name,
        save_dir: save_dir.to_string_lossy().to_string(),
        temp_path: Some(temp_path.to_string_lossy().to_string()),
        final_path: Some(final_path.to_string_lossy().to_string()),
        total_size: probe.total_size,
        downloaded_bytes: 0,
        status: TaskStatus::Queued,
        etag: probe.etag,
        last_modified: probe.last_modified,
        content_type: probe.content_type,
        supports_range: probe.supports_range,
        source_host: probe.source_host,
        connection_count: 0,
        speed_bps: 0,
        health_summary: Some("Queued".to_string()),
        error_message: None,
        created_at: now.clone(),
        updated_at: now,
    };

    db::insert_task_record(&state.pool, &record).await?;
    db::ensure_single_segment_for_task(&state.pool, &record).await?;
    emit_queue_changed(&app);
    start_task_download(app.clone(), state.inner(), record.clone()).await?;

    db::get_task_record(&state.pool, &record.id)
        .await?
        .map(Task::from)
        .ok_or_else(|| "Task was created but could not be loaded.".to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn pause_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Task, String> {
    if let Some(control) = state.downloads.lock().await.remove(&id) {
        control.cancel.store(true, Ordering::SeqCst);
        control.handle.abort();
    }
    db::update_task_status(
        &state.pool,
        &id,
        TaskStatus::Paused,
        0,
        0,
        Some("Paused"),
        None,
    )
    .await?;
    if let Some(segment) = db::get_first_segment_record(&state.pool, &id).await? {
        db::update_segment_status(
            &state.pool,
            &segment.id,
            crate::models::SegmentStatus::Pending,
            None,
            None,
        )
        .await?;
    }
    let task = require_task(&state.pool, &id).await?;
    emit_task_progress_snapshot(&app, &task);
    emit_queue_changed(&app);
    Ok(Task::from(task))
}

#[tauri::command]
#[specta::specta]
pub async fn resume_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Task, String> {
    let task = require_task(&state.pool, &id).await?;
    if matches!(task.status, TaskStatus::Completed) {
        return Err("This download is already completed.".to_string());
    }
    start_task_download(app.clone(), state.inner(), task).await?;
    let task = require_task(&state.pool, &id).await?;
    Ok(Task::from(task))
}

#[tauri::command]
#[specta::specta]
pub async fn retry_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Task, String> {
    if let Some(control) = state.downloads.lock().await.remove(&id) {
        control.cancel.store(true, Ordering::SeqCst);
        control.handle.abort();
    }

    db::update_task_status(
        &state.pool,
        &id,
        TaskStatus::Queued,
        0,
        0,
        Some("Queued"),
        None,
    )
    .await?;
    if let Some(segment) = db::get_first_segment_record(&state.pool, &id).await? {
        db::update_segment_status(
            &state.pool,
            &segment.id,
            crate::models::SegmentStatus::Pending,
            None,
            None,
        )
        .await?;
    }
    let task = require_task(&state.pool, &id).await?;
    start_task_download(app.clone(), state.inner(), task).await?;
    let task = require_task(&state.pool, &id).await?;
    Ok(Task::from(task))
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Task, String> {
    if let Some(control) = state.downloads.lock().await.remove(&id) {
        control.cancel.store(true, Ordering::SeqCst);
        control.handle.abort();
    }
    db::update_task_status(
        &state.pool,
        &id,
        TaskStatus::Failed,
        0,
        0,
        Some("Canceled"),
        Some("Canceled by user."),
    )
    .await?;
    if let Some(segment) = db::get_first_segment_record(&state.pool, &id).await? {
        db::update_segment_status(
            &state.pool,
            &segment.id,
            crate::models::SegmentStatus::Failed,
            None,
            Some("Canceled by user."),
        )
        .await?;
    }
    let task = require_task(&state.pool, &id).await?;
    emit_task_progress_snapshot(&app, &task);
    emit_queue_changed(&app);
    Ok(Task::from(task))
}

#[tauri::command]
#[specta::specta]
pub async fn delete_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    delete_file: bool,
) -> Result<(), String> {
    if let Some(control) = state.downloads.lock().await.remove(&id) {
        control.cancel.store(true, Ordering::SeqCst);
        control.handle.abort();
    }

    if delete_file {
        if let Some(task) = db::get_task_record(&state.pool, &id).await? {
            for path in [task.temp_path, task.final_path].into_iter().flatten() {
                match std::fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(format!("Could not delete {path}: {e}")),
                }
            }
        }
    }

    db::delete_task_record(&state.pool, &id).await?;
    emit_queue_changed(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn open_task_file(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let task = require_task(&state.pool, &id).await?;
    let final_path = task
        .final_path
        .ok_or_else(|| "This task does not have a file path yet.".to_string())?;
    let path = PathBuf::from(final_path);
    if !path.exists() {
        return Err("The downloaded file was not found on disk.".to_string());
    }
    platform::open_path(&path)
}

#[tauri::command]
#[specta::specta]
pub async fn open_task_folder(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let task = require_task(&state.pool, &id).await?;
    let path = task
        .final_path
        .as_deref()
        .and_then(|value| Path::new(value).parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from(task.save_dir));
    if !path.exists() {
        return Err("The download folder was not found on disk.".to_string());
    }
    platform::open_path(&path)
}

pub async fn seed_mock_data(pool: &sqlx::SqlitePool) -> Result<Vec<Task>, String> {
    db::clear_tasks(pool).await?;
    let now = now_iso();
    let mocks = build_mock_tasks(&now);

    for task in &mocks {
        db::insert_task_record(pool, task).await?;
    }

    db::list_task_records(pool)
        .await
        .map(|records| records.into_iter().map(Task::from).collect())
}

#[tauri::command]
#[specta::specta]
pub async fn seed_mock_tasks(state: State<'_, AppState>) -> Result<Vec<Task>, String> {
    seed_mock_data(&state.pool).await
}

async fn start_task_download(
    app: AppHandle,
    state: &AppState,
    task: TaskRecord,
) -> Result<(), String> {
    let task = prepare_task_for_download(&state.pool, task).await?;
    let mut downloads = state.downloads.lock().await;
    if downloads.contains_key(&task.id) {
        return Ok(());
    }

    db::update_task_status(
        &state.pool,
        &task.id,
        TaskStatus::Downloading,
        0,
        1,
        Some("Downloading"),
        None,
    )
    .await?;

    let cancel = Arc::new(AtomicBool::new(false));
    let downloads_map = state.downloads.clone();
    let pool = state.pool.clone();
    let task_id = task.id.clone();
    let map_task_id = task.id.clone();
    let task_app = app.clone();
    let task_cancel = cancel.clone();

    let handle = tokio::spawn(async move {
        let engine = match HttpEngine::new() {
            Ok(engine) => engine,
            Err(error) => {
                mark_download_failed(&task_app, &pool, &task_id, error).await;
                let _ = downloads_map.lock().await.remove(&task_id);
                return;
            }
        };

        let result = engine
            .download(task_app.clone(), pool.clone(), task, task_cancel.clone())
            .await;
        let canceled = task_cancel.load(Ordering::SeqCst);
        let _ = downloads_map.lock().await.remove(&task_id);

        if let Err(error) = result {
            if !canceled {
                mark_download_failed(&task_app, &pool, &task_id, error).await;
            }
        }
    });

    downloads.insert(
        map_task_id,
        DownloadControl {
            cancel,
            handle,
        },
    );

    emit_queue_changed(&app);
    Ok(())
}

async fn prepare_task_for_download(
    pool: &sqlx::SqlitePool,
    task: TaskRecord,
) -> Result<TaskRecord, String> {
    if task.status == TaskStatus::NeedsAttention {
        return Err("Remote file changed. Restart download to avoid corruption.".to_string());
    }

    let segment = db::ensure_single_segment_for_task(pool, &task).await?;
    let temp_path = task
        .temp_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "Task is missing a temporary path.".to_string())?;
    let temp_exists = temp_path.exists();
    let temp_size = std::fs::metadata(&temp_path)
        .map(|metadata| i64::try_from(metadata.len()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    let recorded_progress = task.downloaded_bytes.max(segment.downloaded_until);

    if let Some(message) = local_resume_error(
        recorded_progress,
        temp_exists,
        temp_size,
        task.total_size,
        task.supports_range,
    ) {
        fail_task_and_segment(pool, &task.id, &segment.id, message, temp_size).await?;
        return Err(message.to_string());
    }

    if temp_size > 0 {
        let probe = HttpEngine::new()?.probe(task.final_url.as_deref().unwrap_or(&task.url)).await?;
        if let Some(message) = resume_mismatch_message(&task, &probe) {
            db::update_task_status(
                pool,
                &task.id,
                TaskStatus::NeedsAttention,
                0,
                0,
                Some(&message),
                Some(&message),
            )
            .await?;
            db::update_segment_status(
                pool,
                &segment.id,
                crate::models::SegmentStatus::Failed,
                Some(temp_size),
                Some(&message),
            )
            .await?;
            return Err(message);
        }
    }

    if temp_size > segment.downloaded_until {
        db::update_task_and_segment_progress(
            pool,
            &task.id,
            &segment.id,
            temp_size,
            0,
            0,
            task.status,
        )
        .await?;
    }

    require_task(pool, &task.id).await
}

pub fn local_resume_error(
    recorded_progress: i64,
    temp_exists: bool,
    temp_size: i64,
    total_size: i64,
    supports_range: bool,
) -> Option<&'static str> {
    if temp_size > total_size && total_size > 0 {
        return Some("Temporary file is larger than the remote file.");
    }
    if recorded_progress > 0 && !temp_exists {
        return Some("Temporary file is missing. Restart this download.");
    }
    if recorded_progress > temp_size {
        return Some("Temporary file is smaller than the recorded progress.");
    }
    if temp_size > 0 && !supports_range {
        return Some("Resume unavailable. Restart this download from the beginning.");
    }
    None
}

pub fn resume_mismatch_message(
    task: &TaskRecord,
    probe: &crate::download::ProbeResult,
) -> Option<String> {
    if task.total_size != probe.total_size {
        return Some("Remote file changed. Restart download to avoid corruption.".to_string());
    }
    if !probe.supports_range {
        return Some("Server no longer supports resume. Restart this download.".to_string());
    }
    if task.etag.as_deref().is_some_and(|etag| Some(etag) != probe.etag.as_deref()) {
        return Some("Remote file changed. Restart download to avoid corruption.".to_string());
    }
    if task
        .last_modified
        .as_deref()
        .is_some_and(|last_modified| Some(last_modified) != probe.last_modified.as_deref())
    {
        return Some("Remote file changed. Restart download to avoid corruption.".to_string());
    }
    None
}

async fn fail_task_and_segment(
    pool: &sqlx::SqlitePool,
    task_id: &str,
    segment_id: &str,
    message: &str,
    downloaded_until: i64,
) -> Result<(), String> {
    db::update_task_status(
        pool,
        task_id,
        TaskStatus::Failed,
        0,
        0,
        Some(message),
        Some(message),
    )
    .await?;
    db::update_segment_status(
        pool,
        segment_id,
        crate::models::SegmentStatus::Failed,
        Some(downloaded_until),
        Some(message),
    )
    .await
}

async fn mark_download_failed(app: &AppHandle, pool: &sqlx::SqlitePool, task_id: &str, error: String) {
    let _ = db::update_task_status(
        pool,
        task_id,
        TaskStatus::Failed,
        0,
        0,
        Some(&error),
        Some(&error),
    )
    .await;
    if let Ok(Some(segment)) = db::get_first_segment_record(pool, task_id).await {
        let _ = db::update_segment_status(
            pool,
            &segment.id,
            crate::models::SegmentStatus::Failed,
            None,
            Some(&error),
        )
        .await;
    }
    if let Ok(Some(task)) = db::get_task_record(pool, task_id).await {
        emit_task_progress_snapshot(app, &task);
    }
    emit_queue_changed(app);
}

async fn require_task(pool: &sqlx::SqlitePool, id: &str) -> Result<TaskRecord, String> {
    db::get_task_record(pool, id)
        .await?
        .ok_or_else(|| "Task not found.".to_string())
}

fn emit_task_progress_snapshot(app: &AppHandle, task: &TaskRecord) {
    let payload = crate::models::TaskProgressPayload {
        task_id: task.id.clone(),
        downloaded_bytes: task.downloaded_bytes.to_string(),
        total_size: task.total_size.to_string(),
        speed_bps: task.speed_bps.to_string(),
        connection_count: task.connection_count,
        status: task.status,
    };
    emit_task_progress(app, &payload);
}

fn unique_final_path(save_dir: &Path, requested_file_name: &str) -> PathBuf {
    let sanitized = sanitize_file_name(requested_file_name);
    let candidate = save_dir.join(&sanitized);
    if !candidate.exists()
        && !PathBuf::from(format!("{}.vibe-downloading", candidate.display())).exists()
    {
        return candidate;
    }

    let path = Path::new(&sanitized);
    let stem = path.file_stem().and_then(|value| value.to_str()).unwrap_or("download");
    let extension = path.extension().and_then(|value| value.to_str());

    for index in 1..10_000 {
        let name = match extension {
            Some(extension) if !extension.is_empty() => format!("{stem} ({index}).{extension}"),
            _ => format!("{stem} ({index})"),
        };
        let candidate = save_dir.join(name);
        if !candidate.exists()
            && !PathBuf::from(format!("{}.vibe-downloading", candidate.display())).exists()
        {
            return candidate;
        }
    }

    save_dir.join(format!("download-{}", chrono::Utc::now().timestamp()))
}

fn sanitize_file_name(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        format!("download-{}", chrono::Utc::now().timestamp())
    } else {
        trimmed
    }
}

fn build_mock_tasks(now: &str) -> Vec<TaskRecord> {
    vec![
        mock_task(
            "ubuntu-24.04.iso",
            "https://releases.ubuntu.com/noble/ubuntu-24.04-desktop-amd64.iso",
            "releases.ubuntu.com",
            TaskStatus::Downloading,
            4_200_000_000,
            1_680_000_000,
            8,
            48_500_000,
            Some("Downloading steadily".into()),
            now,
        ),
        mock_task(
            "node-v22.pkg",
            "https://nodejs.org/dist/v22.0.0/node-v22.0.0.pkg",
            "nodejs.org",
            TaskStatus::Downloading,
            80_000_000,
            52_000_000,
            4,
            12_400_000,
            Some("Server limit detected".into()),
            now,
        ),
        mock_task(
            "rust-docs.pdf",
            "https://doc.rust-lang.org/book.pdf",
            "doc.rust-lang.org",
            TaskStatus::Paused,
            12_000_000,
            4_800_000,
            0,
            0,
            None,
            now,
        ),
        mock_task(
            "game-patch.zip",
            "https://cdn.example.com/patches/season-12.zip",
            "cdn.example.com",
            TaskStatus::Queued,
            2_400_000_000,
            0,
            0,
            0,
            None,
            now,
        ),
        mock_task(
            "dataset.tar.gz",
            "https://data.example.org/ml/dataset.tar.gz",
            "data.example.org",
            TaskStatus::Retrying,
            900_000_000,
            120_000_000,
            2,
            3_200_000,
            Some("Network fluctuation, retrying".into()),
            now,
        ),
        mock_task(
            "driver-setup.exe",
            "https://vendor.example.net/drivers/setup.exe",
            "vendor.example.net",
            TaskStatus::Failed,
            350_000_000,
            89_000_000,
            0,
            0,
            Some("Resume unavailable".into()),
            now,
        ),
        mock_task(
            "llm-weights.safetensors",
            "https://models.example.ai/weights/v3.safetensors",
            "models.example.ai",
            TaskStatus::NeedsAttention,
            8_000_000_000,
            2_100_000_000,
            0,
            0,
            Some("Remote file changed. Restart download to avoid corruption.".into()),
            now,
        ),
        mock_task(
            "archlinux.iso",
            "https://mirror.archlinux.org/iso/latest/archlinux-x86_64.iso",
            "mirror.archlinux.org",
            TaskStatus::Completed,
            1_300_000_000,
            1_300_000_000,
            0,
            0,
            Some("Completed".into()),
            now,
        ),
        mock_task(
            "fonts-bundle.zip",
            "https://github.com/google/fonts/archive/refs/heads/main.zip",
            "github.com",
            TaskStatus::WaitingNetwork,
            220_000_000,
            45_000_000,
            0,
            0,
            Some("Waiting for network".into()),
            now,
        ),
        mock_task(
            "vscode.deb",
            "https://code.visualstudio.com/sha/download?build=stable&os=linux-deb-x64",
            "code.visualstudio.com",
            TaskStatus::Downloading,
            95_000_000,
            71_000_000,
            2,
            8_900_000,
            Some("Disk write slower than network".into()),
            now,
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn mock_task(
    file_name: &str,
    url: &str,
    host: &str,
    status: TaskStatus,
    total_size: i64,
    downloaded_bytes: i64,
    connection_count: i32,
    speed_bps: i64,
    health_summary: Option<String>,
    now: &str,
) -> TaskRecord {
    let error_message = if matches!(status, TaskStatus::Failed | TaskStatus::NeedsAttention) {
        health_summary.clone()
    } else {
        None
    };

    TaskRecord {
        id: Uuid::new_v4().to_string(),
        url: url.to_string(),
        final_url: Some(url.to_string()),
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
        supports_range: true,
        source_host: host.to_string(),
        connection_count,
        speed_bps,
        health_summary,
        error_message,
        created_at: now.to_string(),
        updated_at: now.to_string(),
    }
}
