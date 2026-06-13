use tauri::State;
use uuid::Uuid;

use crate::{
    db,
    models::{task::now_iso, HashVerificationStatus, Task, TaskFileRecord, TaskRecord, TaskStatus},
    AppState,
};

use super::task_from_record_with_files;

#[cfg(debug_assertions)]
pub async fn seed_mock_data(pool: &sqlx::SqlitePool) -> Result<Vec<Task>, String> {
    db::clear_tasks(pool).await?;
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
    }
}
