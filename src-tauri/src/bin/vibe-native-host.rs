use std::{
    env, fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

use reqwest::Url;
use serde::Serialize;
use tauri_app_lib::models::{BrowserHandoffInput, BrowserKind};

const MAX_MESSAGE_BYTES: u32 = 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeHostResponse {
    status: String,
    request_id: Option<String>,
    handoff_file: Option<String>,
    app_started: bool,
    error_message: Option<String>,
}

fn main() {
    let response = match read_native_message().and_then(handle_message) {
        Ok(response) => response,
        Err(error) => NativeHostResponse {
            status: "failed".to_string(),
            request_id: None,
            handoff_file: None,
            app_started: false,
            error_message: Some(error),
        },
    };

    if let Err(error) = write_native_message(&response) {
        let _ = writeln!(io::stderr(), "{error}");
    }
}

fn read_native_message() -> Result<BrowserHandoffInput, String> {
    let mut length = [0_u8; 4];
    io::stdin()
        .read_exact(&mut length)
        .map_err(|e| format!("Could not read native message length: {e}"))?;
    let length = u32::from_le_bytes(length);
    if length == 0 || length > MAX_MESSAGE_BYTES {
        return Err("Native message length is outside the accepted range.".to_string());
    }

    let mut buffer = vec![0_u8; length as usize];
    io::stdin()
        .read_exact(&mut buffer)
        .map_err(|e| format!("Could not read native message body: {e}"))?;
    serde_json::from_slice(&buffer).map_err(|e| format!("Invalid native message JSON: {e}"))
}

fn handle_message(input: BrowserHandoffInput) -> Result<NativeHostResponse, String> {
    validate_handoff(&input)?;
    let request_id = input.request_id.trim().to_string();
    let handoff_file = write_handoff_file(&input)?;
    let app_started = start_app(&handoff_file);

    Ok(NativeHostResponse {
        status: "accepted".to_string(),
        request_id: Some(request_id),
        handoff_file: Some(handoff_file.to_string_lossy().to_string()),
        app_started,
        error_message: None,
    })
}

fn validate_handoff(input: &BrowserHandoffInput) -> Result<(), String> {
    if input.version != 1 {
        return Err("Unsupported handoff payload version.".to_string());
    }
    if input.request_id.trim().is_empty() {
        return Err("Handoff request id is required.".to_string());
    }
    if input.action != "download_url" {
        return Err("Unsupported handoff action.".to_string());
    }
    if matches!(input.browser, BrowserKind::Safari) && !cfg!(target_os = "macos") {
        return Err("Safari handoff is only supported on macOS.".to_string());
    }

    let url = Url::parse(input.url.trim()).map_err(|_| "Handoff URL is invalid.".to_string())?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("Only HTTP and HTTPS handoff URLs are accepted.".to_string());
    }
    if url.username() != "" || url.password().is_some() {
        return Err("Handoff URLs must not contain embedded credentials.".to_string());
    }

    Ok(())
}

fn write_handoff_file(input: &BrowserHandoffInput) -> Result<PathBuf, String> {
    let dir = env::var_os("VIBE_DOWNLOADER_HANDOFF_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir().join("vibe-downloader-handoff"));
    fs::create_dir_all(&dir).map_err(|e| format!("Could not create handoff directory: {e}"))?;

    let file_name = safe_file_stem(&input.request_id);
    let path = dir.join(format!("{file_name}.json"));
    let raw = serde_json::to_vec_pretty(input).map_err(|e| e.to_string())?;
    fs::write(&path, raw).map_err(|e| format!("Could not write handoff file: {e}"))?;
    Ok(path)
}

fn start_app(handoff_file: &Path) -> bool {
    let Some(app_path) = app_executable_path() else {
        return false;
    };
    Command::new(app_path)
        .arg("--browser-handoff-file")
        .arg(handoff_file)
        .spawn()
        .is_ok()
}

fn app_executable_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os("VIBE_DOWNLOADER_APP_EXE").map(PathBuf::from) {
        if path.exists() {
            return Some(path);
        }
    }

    let current = env::current_exe().ok()?;
    let app_name = if cfg!(target_os = "windows") {
        "vibe-downloader.exe"
    } else {
        "vibe-downloader"
    };
    let sibling = current.with_file_name(app_name);
    sibling.exists().then_some(sibling)
}

fn write_native_message(response: &NativeHostResponse) -> Result<(), String> {
    let raw = serde_json::to_vec(response).map_err(|e| e.to_string())?;
    let length = u32::try_from(raw.len())
        .map_err(|_| "Native response is too large to write.".to_string())?;
    let mut stdout = io::stdout();
    stdout
        .write_all(&length.to_le_bytes())
        .and_then(|_| stdout.write_all(&raw))
        .and_then(|_| stdout.flush())
        .map_err(|e| format!("Could not write native response: {e}"))
}

fn safe_file_stem(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "handoff".to_string()
    } else {
        cleaned
    }
}
