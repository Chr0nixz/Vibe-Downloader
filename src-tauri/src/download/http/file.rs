use std::path::{Path, PathBuf};

use sqlx::SqlitePool;
use tokio::fs;

use crate::{db, models::AppErrorPayload};

pub(super) async fn finalize_download_file(
    temp_path: &Path,
    preferred_final_path: &Path,
) -> Result<PathBuf, String> {
    let final_path = available_final_path(preferred_final_path).await?;
    fs::rename(temp_path, &final_path).await.map_err(|e| {
        AppErrorPayload::disk_write_failed(format!("Could not finalize the downloaded file: {e}"))
            .command_error()
    })?;
    Ok(final_path)
}

async fn available_final_path(preferred_final_path: &Path) -> Result<PathBuf, String> {
    if !fs::try_exists(preferred_final_path).await.unwrap_or(false) {
        return Ok(preferred_final_path.to_path_buf());
    }

    let parent = preferred_final_path
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let stem = preferred_final_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("download");
    let extension = preferred_final_path
        .extension()
        .and_then(|value| value.to_str());

    for index in 1..10_000 {
        let file_name = match extension {
            Some(extension) if !extension.is_empty() => format!("{stem} ({index}).{extension}"),
            _ => format!("{stem} ({index})"),
        };
        let candidate = parent.join(file_name);
        if !fs::try_exists(&candidate).await.unwrap_or(false) {
            return Ok(candidate);
        }
    }

    Err(
        AppErrorPayload::final_path_conflict(&preferred_final_path.to_string_lossy())
            .command_error(),
    )
}

pub(super) async fn persist_completed_path(
    pool: &SqlitePool,
    task_id: &str,
    completed_path: &Path,
) -> Result<(), String> {
    let file_name = completed_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("download")
        .to_string();
    db::update_task_final_path(pool, task_id, &file_name, &completed_path.to_string_lossy()).await
}
