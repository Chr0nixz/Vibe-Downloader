use std::{collections::HashSet, time::Duration};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::{db, events::emit_clipboard_link_detected, models::task::now_iso, AppState};

const POLL_INTERVAL: Duration = Duration::from_millis(1000);
const MAX_CLIPBOARD_TEXT_LEN: usize = 64 * 1024;
const URL_PREFIXES: [&str; 9] = [
    "http://",
    "https://",
    "ftp://",
    "ftps://",
    "sftp://",
    "webdav://",
    "webdavs://",
    "magnet:",
    "file://",
];

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardLinkDetectedPayload {
    pub id: String,
    pub urls: Vec<String>,
    pub primary_url: String,
    pub detected_at: String,
}

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        run_monitor(app).await;
    });
}

async fn run_monitor(app: AppHandle) {
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut enabled_last_tick = false;
    let mut last_text: Option<String> = None;

    loop {
        interval.tick().await;

        let Some(state) = app.try_state::<AppState>() else {
            continue;
        };
        if state
            .quit_requested
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            tracing::debug!("clipboard monitor exiting (shutdown requested)");
            break;
        }
        let enabled = match db::clipboard_monitor_enabled(&state.pool).await {
            Ok(enabled) => enabled,
            Err(error) => {
                tracing::warn!(error = %error, "failed to read clipboard monitor setting");
                false
            }
        };

        if !enabled {
            enabled_last_tick = false;
            continue;
        }

        let text = match app.clipboard().read_text() {
            Ok(text) => text,
            Err(error) => {
                tracing::debug!(error = %error, "clipboard text read skipped");
                continue;
            }
        };

        if !enabled_last_tick {
            last_text = Some(text);
            enabled_last_tick = true;
            continue;
        }

        if last_text.as_deref() == Some(text.as_str()) {
            continue;
        }
        last_text = Some(text.clone());

        let urls = extract_download_urls(&text);
        if urls.is_empty() {
            continue;
        }

        let payload = ClipboardLinkDetectedPayload {
            id: uuid::Uuid::new_v4().to_string(),
            primary_url: urls[0].clone(),
            urls,
            detected_at: now_iso(),
        };
        emit_clipboard_link_detected(&app, &payload);
    }
}

pub fn extract_download_urls(text: &str) -> Vec<String> {
    if text.trim().is_empty() || text.len() > MAX_CLIPBOARD_TEXT_LEN {
        return Vec::new();
    }

    // E-9: Cheap short-circuit. All supported protocol URLs contain ":" (http://, magnet:, file://, etc.).
    // If the text has no ":", return immediately to avoid a full to_ascii_lowercase allocation
    // + 9 substring searches on 64KB of plain text. The ":" check is case-sensitive and letter-free, so it works for all case variants.
    // False positives (plain text with a colon) go through the full path and are filtered by normalize_download_url with no functional impact.
    if !text.contains(':') {
        return Vec::new();
    }

    let mut starts = Vec::new();
    let lower = text.to_ascii_lowercase();
    for prefix in URL_PREFIXES {
        let mut offset = 0;
        while let Some(index) = lower[offset..].find(prefix) {
            let start = offset + index;
            if start == 0 || !lower.as_bytes()[start - 1].is_ascii_alphanumeric() {
                starts.push(start);
            }
            offset = start + prefix.len();
        }
    }
    starts.sort_unstable();
    starts.dedup();

    let mut seen = HashSet::new();
    let mut urls = Vec::new();
    for start in starts {
        let raw = candidate_at(text, start);
        if let Some(url) = normalize_download_url(raw) {
            if seen.insert(url.clone()) {
                urls.push(url);
            }
        }
    }
    urls
}

fn candidate_at(text: &str, start: usize) -> &str {
    let mut end = text.len();
    for (offset, ch) in text[start..].char_indices() {
        if ch.is_whitespace() || matches!(ch, '<' | '>' | '"' | '\'') {
            end = start + offset;
            break;
        }
    }
    trim_candidate(&text[start..end])
}

fn trim_candidate(value: &str) -> &str {
    value.trim_matches(|ch: char| {
        matches!(
            ch,
            ',' | '.' | ';' | '!' | '?' | ')' | ']' | '}' | '"' | '\''
        )
    })
}

fn normalize_download_url(raw: &str) -> Option<String> {
    let parsed = Url::parse(raw).ok()?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }

    match parsed.scheme() {
        "http" | "https" | "ftp" | "ftps" | "sftp" | "webdav" | "webdavs" | "magnet" => {
            Some(parsed.to_string())
        }
        "file" if is_local_manifest_path(parsed.path()) => Some(parsed.to_string()),
        _ => None,
    }
}

fn is_local_manifest_path(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    path.ends_with(".torrent")
        || path.ends_with(".meta4")
        || path.ends_with(".metalink")
        || path.ends_with(".mpd")
}
