//! Cross-protocol shared file operations: temp file preallocation, final path
//! resolution, completed path persistence.
//!
//! Originally in `download/http/file.rs`; moved to a protocol-neutral location
//! because it is reused by FTP/SFTP/DASH/HLS/Metalink and other HTTP-derived engines.

use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};

use sqlx::SqlitePool;
use tokio::fs;
use uuid::Uuid;

use crate::{db, models::AppErrorPayload};

const TEMP_DOWNLOAD_SUFFIX: &str = ".vibe-downloading";

/// Publish a completed download to its reserved final path without clobbering.
///
/// Same-volume: rename only when the destination does not already exist.
/// Cross-volume: copy into a unique staging file beside the destination, fsync,
/// then atomic rename onto the final path. Never copies directly over an existing
/// final file.
pub(crate) async fn finalize_download_file(
    temp_path: &Path,
    preferred_final_path: &Path,
) -> Result<PathBuf, String> {
    if fs::try_exists(preferred_final_path).await.unwrap_or(false) {
        return Err(
            AppErrorPayload::final_path_conflict(&preferred_final_path.to_string_lossy())
                .command_error(),
        );
    }

    match fs::rename(temp_path, preferred_final_path).await {
        Ok(()) => {
            fsync_path(preferred_final_path).await;
            Ok(preferred_final_path.to_path_buf())
        }
        Err(error) if is_cross_device_error(&error) => {
            publish_across_volumes(temp_path, preferred_final_path).await?;
            Ok(preferred_final_path.to_path_buf())
        }
        Err(error) => {
            // Destination may have appeared between the exists check and rename.
            if fs::try_exists(preferred_final_path).await.unwrap_or(false) {
                return Err(AppErrorPayload::final_path_conflict(
                    &preferred_final_path.to_string_lossy(),
                )
                .command_error());
            }
            Err(AppErrorPayload::disk_write_failed(format!(
                "Could not move downloaded file to {dest}: {error}",
                dest = preferred_final_path.display()
            ))
            .command_error())
        }
    }
}

async fn publish_across_volumes(
    temp_path: &Path,
    preferred_final_path: &Path,
) -> Result<(), String> {
    if fs::try_exists(preferred_final_path).await.unwrap_or(false) {
        return Err(
            AppErrorPayload::final_path_conflict(&preferred_final_path.to_string_lossy())
                .command_error(),
        );
    }

    let token = publish_token(temp_path, preferred_final_path);
    let staging_path = PathBuf::from(format!(
        "{}.{token}.staging",
        preferred_final_path.display()
    ));
    if fs::try_exists(&staging_path).await.unwrap_or(false) {
        return Err(
            AppErrorPayload::final_path_conflict(&preferred_final_path.to_string_lossy())
                .command_error(),
        );
    }

    fs::copy(temp_path, &staging_path).await.map_err(|e| {
        AppErrorPayload::disk_write_failed(format!(
            "Could not copy downloaded file to staging {dest}: {e}",
            dest = staging_path.display()
        ))
        .command_error()
    })?;
    fsync_path(&staging_path).await;

    if fs::try_exists(preferred_final_path).await.unwrap_or(false) {
        let _ = fs::remove_file(&staging_path).await;
        return Err(
            AppErrorPayload::final_path_conflict(&preferred_final_path.to_string_lossy())
                .command_error(),
        );
    }

    fs::rename(&staging_path, preferred_final_path)
        .await
        .map_err(|e| {
            AppErrorPayload::disk_write_failed(format!(
                "Could not publish staged file to {dest}: {e}",
                dest = preferred_final_path.display()
            ))
            .command_error()
        })?;
    fsync_path(preferred_final_path).await;

    if let Err(error) = fs::remove_file(temp_path).await {
        tracing::warn!(
            path = %temp_path.display(),
            error = %error,
            "could not remove temp file after cross-volume publish"
        );
    }
    Ok(())
}

/// Prefer the task UUID embedded in `{final}.{task_id}.vibe-downloading`.
fn publish_token(temp_path: &Path, preferred_final_path: &Path) -> String {
    let temp_name = temp_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let final_name = preferred_final_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let prefix = format!("{final_name}.");
    if let Some(middle) = temp_name
        .strip_prefix(&prefix)
        .and_then(|rest| rest.strip_suffix(TEMP_DOWNLOAD_SUFFIX))
    {
        if !middle.is_empty() {
            return middle.to_string();
        }
    }
    Uuid::new_v4().to_string()
}

async fn fsync_path(path: &Path) {
    // E-11: fsync after publish so a crash cannot leave metadata committed
    // without durable file data. Failure is non-fatal (warn only).
    if let Ok(file) = fs::File::open(path).await {
        if let Err(error) = file.sync_all().await {
            tracing::warn!(
                path = %path.display(),
                error = %error,
                "fsync after finalize failed (data may not be durable)"
            );
        }
    }
}

fn is_cross_device_error(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::CrossesDevices)
        || matches!(error.raw_os_error(), Some(17) | Some(18))
}

pub(crate) async fn preallocate_temp_file(file: &fs::File, total_size: i64, task_id: &str) {
    if total_size <= 0 {
        return;
    }
    if let Err(error) = file
        .set_len(u64::try_from(total_size).unwrap_or(u64::MAX))
        .await
    {
        tracing::warn!(
            task_id,
            total_size,
            error = %error,
            "failed to preallocate temporary download file"
        );
    }
}

pub(crate) async fn persist_completed_path(
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
