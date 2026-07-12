//! 跨协议共享的文件操作工具：临时文件预分配、最终路径落定、完成路径持久化。
//!
//! 原位于 `download/http/file.rs`，因其被 FTP/SFTP/DASH/HLS/Metalink 等非
//! HTTP 引擎复用，上移至协议中立位置。

use std::path::{Path, PathBuf};

use sqlx::SqlitePool;
use tokio::fs;

use crate::{db, models::AppErrorPayload};

pub(crate) async fn finalize_download_file(
    temp_path: &Path,
    preferred_final_path: &Path,
) -> Result<PathBuf, String> {
    let final_path = available_final_path(preferred_final_path).await?;
    if fs::rename(temp_path, &final_path).await.is_ok() {
        // E-11: fsync the final path after rename to ensure data is durable on
        // disk. Without this, a crash/power loss after rename could leave the
        // file system metadata committed but file data not yet flushed to the
        // storage device. fsync failure is non-fatal (warn only) — same policy
        // as the cross-drive copy path below.
        if let Ok(file) = fs::File::open(&final_path).await {
            if let Err(error) = file.sync_all().await {
                tracing::warn!(
                    final_path = %final_path.display(),
                    error = %error,
                    "fsync after finalize failed (data may not be durable)"
                );
            }
        }
        return Ok(final_path);
    }
    // Cross-drive fallback: rename fails with EXDEV when source and dest are
    // on different volumes (e.g. C: → D: on Windows). Copy, fsync, then delete.
    fs::copy(temp_path, &final_path).await.map_err(|e| {
        AppErrorPayload::disk_write_failed(format!(
            "Could not copy downloaded file to {dest}: {e}",
            dest = final_path.display()
        ))
        .command_error()
    })?;
    // fsync the destination to ensure data is on disk before removing the source.
    // If sync fails, keep the temp file as a safety net — deleting the source
    // without confirmed persistence risks data loss on network/unreliable drives.
    let synced = match fs::File::open(&final_path).await {
        Ok(file) => match file.sync_all().await {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(
                    path = %final_path.display(),
                    error = %error,
                    "sync_all failed after cross-drive copy; keeping temp file as safety net"
                );
                false
            }
        },
        Err(error) => {
            tracing::warn!(
                path = %final_path.display(),
                error = %error,
                "could not open copied file for sync_all"
            );
            false
        }
    };
    if synced {
        if let Err(error) = fs::remove_file(temp_path).await {
            tracing::warn!(
                path = %temp_path.display(),
                error = %error,
                "could not remove temp file after cross-drive copy"
            );
        }
    }
    Ok(final_path)
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
