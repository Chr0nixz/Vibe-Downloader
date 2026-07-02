//! Tauri command for probing ffmpeg availability and version.
//!
//! The Settings UI uses this to verify a user-configured `ffmpeg_path` and to
//! surface a status badge ("detected" / "invalid") next to the path input.

use std::path::PathBuf;

use tauri::State;

use crate::download::ffmpeg::{ffmpeg_path, probe_ffmpeg_version_at_path};
use crate::models::{AppErrorPayload, RecoveryAction};
use crate::AppState;

/// Probe the ffmpeg binary version.
///
/// Accepts an optional path string. When omitted, resolves the path from
/// app state using the full resolution chain
/// (`VIBE_FFMPEG_PATH` > `ffmpeg_path` setting > PATH lookup).
///
/// Returns the first line of `ffmpeg -version` output on success.
#[tauri::command]
#[specta::specta]
pub async fn probe_ffmpeg_version(
    state: State<'_, AppState>,
    path: Option<String>,
) -> Result<String, String> {
    let resolved = match path {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p.trim()),
        _ => ffmpeg_path(Some(&state.pool))
            .await
            .ok_or_else(|| {
                AppErrorPayload::new(
                    "ffmpeg_not_found",
                    "ffmpeg was not found. Set a path in Settings → External tools, or install ffmpeg on PATH.",
                    true,
                    vec![RecoveryAction::ConfigureFfmpeg.as_str()],
                )
                .command_error()
            })?,
    };
    probe_ffmpeg_version_at_path(&resolved).await
}
