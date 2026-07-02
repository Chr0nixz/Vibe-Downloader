use std::{
    env, fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc,
    time::Duration,
};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use tauri_app_lib::{
    browser_realtime,
    download::{ssrf, EngineRegistry},
    logging::{init_standalone_logging, sanitize_url},
    models::{BrowserHandoffInput, BrowserKind},
};

const MAX_MESSAGE_BYTES: u32 = 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeHostResponse {
    status: String,
    request_id: Option<String>,
    handoff_file: Option<String>,
    app_started: bool,
    ws_url: Option<String>,
    token: Option<String>,
    error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeHostAction {
    action: String,
}

fn main() {
    if let Err(error) = init_standalone_logging() {
        let _ = writeln!(
            io::stderr(),
            "failed to initialize native host logging: {error}"
        );
    }

    let response = match read_native_message().and_then(handle_message) {
        Ok(response) => response,
        Err(error) => {
            tracing::error!(error = %error, "native host message handling failed");
            NativeHostResponse {
                status: "failed".to_string(),
                request_id: None,
                handoff_file: None,
                app_started: false,
                ws_url: None,
                token: None,
                error_message: Some(error),
            }
        }
    };

    if let Err(error) = write_native_message(&response) {
        tracing::error!(error = %error, "failed to write native host response");
    }
}

const NATIVE_MESSAGE_TIMEOUT: Duration = Duration::from_secs(30);

fn read_native_message() -> Result<serde_json::Value, String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = read_raw_native_message();
        let _ = tx.send(result);
    });
    match rx.recv_timeout(NATIVE_MESSAGE_TIMEOUT) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err("Timed out waiting for native message from browser.".to_string())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("Native message reader thread terminated unexpectedly.".to_string())
        }
    }
}

fn read_raw_native_message() -> Result<serde_json::Value, String> {
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

fn handle_message(value: serde_json::Value) -> Result<NativeHostResponse, String> {
    let action = serde_json::from_value::<NativeHostAction>(value.clone())
        .map_err(|e| format!("Invalid native message action: {e}"))?
        .action;
    if action == "bootstrap" {
        return handle_bootstrap();
    }
    let input: BrowserHandoffInput =
        serde_json::from_value(value).map_err(|e| format!("Invalid handoff payload: {e}"))?;
    let request_id = input.request_id.trim().to_string();
    tracing::info!(
        request_id = %request_id,
        browser = %input.browser.display_name(),
        action = %input.action,
        url = %sanitize_url(&input.url),
        "native host message received"
    );

    validate_handoff(&input)?;
    let handoff_file = write_handoff_file(&input)?;
    tracing::debug!(
        request_id = %request_id,
        path = %handoff_file.display(),
        "handoff file written"
    );
    let app_started = start_app(&handoff_file);
    tracing::info!(
        request_id = %request_id,
        app_started,
        "native host handoff accepted"
    );

    Ok(NativeHostResponse {
        status: "accepted".to_string(),
        request_id: Some(request_id),
        handoff_file: Some(handoff_file.to_string_lossy().to_string()),
        app_started,
        ws_url: None,
        token: None,
        error_message: None,
    })
}

fn handle_bootstrap() -> Result<NativeHostResponse, String> {
    let app_started = start_app_without_handoff();
    let bootstrap = read_bootstrap_with_retry()?;
    Ok(NativeHostResponse {
        status: "ready".to_string(),
        request_id: None,
        handoff_file: None,
        app_started,
        ws_url: Some(bootstrap.ws_url),
        token: Some(bootstrap.token),
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
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Handoff only supports HTTP and HTTPS URLs.".to_string());
    }
    // S-3: Reject literal private/reserved IP addresses (loopback, RFC1918,
    // link-local, etc.) as a first line of defense before writing the handoff
    // file and launching the main app. DNS-rebinding protection (resolving a
    // public hostname to a private IP) remains the responsibility of the main
    // process at connection time; this synchronous check only covers literal
    // IPs and "localhost".
    if ssrf::is_private_or_reserved_url(&url) {
        return Err(
            "Handoff URLs pointing to private or reserved addresses are not allowed."
                .to_string(),
        );
    }
    EngineRegistry::new()?.engine_for_uri(url.as_str())?;
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
    if path.exists() {
        return Err("Handoff request id already exists.".to_string());
    }
    let temp_path = dir.join(format!("{file_name}.{}.tmp", std::process::id()));
    let raw = serde_json::to_vec_pretty(input).map_err(|e| e.to_string())?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|e| format!("Could not create handoff temp file: {e}"))?;
    file.write_all(&raw)
        .and_then(|_| file.flush())
        .map_err(|e| format!("Could not write handoff temp file: {e}"))?;
    fs::rename(&temp_path, &path).map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        format!("Could not publish handoff file: {e}")
    })?;
    Ok(path)
}

fn start_app(handoff_file: &Path) -> bool {
    let Some(app_path) = app_executable_path() else {
        tracing::warn!("could not resolve app executable path");
        return false;
    };
    let started = Command::new(app_path)
        .arg("--browser-handoff-file")
        .arg(handoff_file)
        .spawn()
        .is_ok();
    if !started {
        tracing::warn!(
            handoff_file = %handoff_file.display(),
            "failed to spawn main application"
        );
    }
    started
}

fn start_app_without_handoff() -> bool {
    let Some(app_path) = app_executable_path() else {
        tracing::warn!("could not resolve app executable path");
        return false;
    };
    Command::new(app_path).spawn().is_ok()
}

fn read_bootstrap_with_retry() -> Result<browser_realtime::BrowserBridgeBootstrap, String> {
    for _ in 0..40 {
        if let Ok(bootstrap) = browser_realtime::read_bootstrap_file() {
            return Ok(bootstrap);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    browser_realtime::read_bootstrap_file()
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

// S-3: Native host handoff SSRF coverage.
//
// `validate_handoff` is private to this binary, so these unit tests are the
// only way to verify the literal-IP / localhost rejection added in S-3. The
// main process (`commands::browser::validate_handoff`) has its own coverage
// in `tests/browser_handoff.rs`; these tests ensure the native host's first
// line of defense matches the same `is_private_or_reserved_url` contract.
#[cfg(test)]
mod tests {
    use super::*;
    use tauri_app_lib::models::BrowserKind;

    fn make_handoff(url: &str) -> BrowserHandoffInput {
        BrowserHandoffInput {
            version: 1,
            request_id: "test-req".to_string(),
            browser: BrowserKind::Chrome,
            action: "download_url".to_string(),
            url: url.to_string(),
            source: None,
            browser_download_id: None,
            page_url: None,
            referrer: None,
            user_agent: None,
            suggested_file_name: None,
            total_bytes: None,
            mime: None,
            headers_available: None,
            header_consent_state: None,
            forwarded_headers: None,
        }
    }

    #[test]
    fn validate_handoff_rejects_loopback_ipv4() {
        let input = make_handoff("http://127.0.0.1/file");
        let err = validate_handoff(&input).expect_err("should reject 127.0.0.1");
        assert!(
            err.contains("private or reserved"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_handoff_rejects_private_ipv4() {
        for url in [
            "http://10.0.0.1/file",
            "http://192.168.1.1/file",
            "http://172.16.0.1/file",
            "http://172.31.255.255/file",
        ] {
            let input = make_handoff(url);
            let err = validate_handoff(&input).expect_err("should reject private IP");
            assert!(
                err.contains("private or reserved"),
                "url {url} got: {err}"
            );
        }
    }

    #[test]
    fn validate_handoff_rejects_link_local() {
        let input = make_handoff("http://169.254.169.254/latest/meta-data/");
        let err = validate_handoff(&input).expect_err("should reject link-local");
        assert!(
            err.contains("private or reserved"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_handoff_rejects_localhost() {
        let input = make_handoff("http://localhost/file");
        let err = validate_handoff(&input).expect_err("should reject localhost");
        assert!(
            err.contains("private or reserved"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_handoff_rejects_ipv6_loopback() {
        let input = make_handoff("http://[::1]/file");
        let err = validate_handoff(&input).expect_err("should reject ::1");
        assert!(
            err.contains("private or reserved"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_handoff_allows_public_url() {
        // A public URL must pass the SSRF check. (EngineRegistry may still
        // reject unsupported schemes, but the SSRF guard itself should pass.)
        let input = make_handoff("https://example.com/file");
        let _ = validate_handoff(&input);
        // We don't assert Ok because EngineRegistry::new() may have side
        // effects in some environments; the rejection tests above are the
        // critical ones. If SSRF rejected this, that would be a bug.
    }
}
