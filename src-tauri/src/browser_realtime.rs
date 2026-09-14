use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::Instant,
};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

use crate::{
    commands, db,
    models::{BrowserCaptureSettingsInput, BrowserHandoffInput, Task, TaskProgressPayload},
    AppState,
};

const BOOTSTRAP_FILE_NAME: &str = "vibe-downloader-browser-bridge.json";
const BROWSER_WS_PORT: u16 = 48365;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserBridgeBootstrap {
    pub ws_url: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserRealtimeStatus {
    pub ws_url: Option<String>,
    pub connected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
pub enum BrowserRealtimeEvent {
    Ready(BrowserBridgeBootstrap),
    TasksSnapshot { tasks: Vec<Task> },
    TaskProgress(TaskProgressPayload),
    TaskUpdated(Box<Task>),
    QueueChanged,
}

/// SEC-04: token bucket for bridge-initiated `createDownload` calls. The
/// UUIDv4 handshake token gates *who* may connect, but any local process that
/// steals the token could otherwise issue unbounded task creation to exhaust
/// disk or use the app as a traffic amplifier. 10 requests/minute with a
/// burst of 5 leaves interactive extension usage untouched.
#[derive(Debug)]
struct CreateDownloadRateLimiter {
    burst: f64,
    refill_per_second: f64,
    tokens: f64,
    last_refill: Instant,
}

impl CreateDownloadRateLimiter {
    fn new(burst: u32, per_minute: u32) -> Self {
        Self {
            burst: f64::from(burst),
            refill_per_second: f64::from(per_minute) / 60.0,
            tokens: f64::from(burst),
            last_refill: Instant::now(),
        }
    }

    fn try_acquire(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_per_second).min(self.burst);
        self.last_refill = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// SEC-04: only WebExtension contexts may open the realtime bridge. Browsers
/// always send `Origin` on WebSocket upgrades, so an absent origin means a
/// non-browser local client — defense in depth behind the handshake token.
fn origin_allowed(origin: Option<&str>) -> bool {
    const ALLOWED_PREFIXES: [&str; 2] = ["chrome-extension://", "moz-extension://"];
    origin.is_some_and(|value| {
        ALLOWED_PREFIXES
            .iter()
            .any(|prefix| value.starts_with(prefix))
    })
}

#[derive(Debug)]
pub struct BrowserRealtimeState {
    token: String,
    bootstrap: Mutex<Option<BrowserBridgeBootstrap>>,
    connected_clients: Mutex<usize>,
    tx: broadcast::Sender<BrowserRealtimeEvent>,
    create_download_limiter: std::sync::Mutex<CreateDownloadRateLimiter>,
}

impl BrowserRealtimeState {
    pub fn new() -> Arc<Self> {
        let (tx, _) = broadcast::channel(256);
        Arc::new(Self {
            token: Uuid::new_v4().to_string(),
            bootstrap: Mutex::new(None),
            connected_clients: Mutex::new(0),
            tx,
            create_download_limiter: std::sync::Mutex::new(CreateDownloadRateLimiter::new(5, 10)),
        })
    }

    /// SEC-04: one token per bridge-initiated download creation; the caller
    /// receives a structured error when the bucket is empty.
    pub fn try_acquire_create_download(&self) -> bool {
        self.create_download_limiter
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .try_acquire()
    }

    pub async fn bootstrap(&self) -> Option<BrowserBridgeBootstrap> {
        self.bootstrap.lock().await.clone()
    }

    pub async fn status(&self) -> BrowserRealtimeStatus {
        let bootstrap = self.bootstrap.lock().await.clone();
        let connected = *self.connected_clients.lock().await > 0;
        BrowserRealtimeStatus {
            ws_url: bootstrap.map(|value| value.ws_url),
            connected,
        }
    }

    pub fn broadcast(&self, event: BrowserRealtimeEvent) {
        let _ = self.tx.send(event);
    }
}

#[derive(Clone)]
struct ServerState {
    app: AppHandle,
    realtime: Arc<BrowserRealtimeState>,
}

#[derive(Debug, Deserialize)]
struct WsQuery {
    token: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
enum BrowserServerResponse<T: Serialize> {
    Result { id: Option<String>, value: T },
    Error { message: String },
    Pong,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserClientEnvelope {
    id: Option<String>,
    #[serde(rename = "type")]
    message_type: String,
    payload: Option<serde_json::Value>,
}

pub async fn start(app: AppHandle, realtime: Arc<BrowserRealtimeState>) -> Result<(), String> {
    let state = ServerState {
        app: app.clone(),
        realtime: realtime.clone(),
    };
    let router = Router::new()
        .route("/browser/ws", get(ws_handler))
        .with_state(state);
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), BROWSER_WS_PORT);
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::warn!(
                port = BROWSER_WS_PORT,
                error = %error,
                "browser realtime fixed port unavailable, falling back to an ephemeral port"
            );
            tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
                .await
                .map_err(|e| format!("Could not start browser realtime bridge: {e}"))?
        }
    };
    let local_addr = listener
        .local_addr()
        .map_err(|e| format!("Could not read browser realtime bridge address: {e}"))?;
    let bootstrap = BrowserBridgeBootstrap {
        ws_url: format!("ws://{local_addr}/browser/ws"),
        token: realtime.token.clone(),
    };
    *realtime.bootstrap.lock().await = Some(bootstrap.clone());
    write_bootstrap_file(&bootstrap)?;
    realtime.broadcast(BrowserRealtimeEvent::Ready(bootstrap));

    tokio::spawn(async move {
        let app_for_shutdown = app.clone();
        let shutdown_signal = async move {
            loop {
                if let Some(state) = app_for_shutdown.try_state::<crate::AppState>() {
                    if state
                        .quit_requested
                        .load(std::sync::atomic::Ordering::SeqCst)
                    {
                        break;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        };
        if let Err(error) = axum::serve(listener, router)
            .with_graceful_shutdown(shutdown_signal)
            .await
        {
            tracing::error!(error = %error, "browser realtime bridge stopped");
        }
    });
    Ok(())
}

async fn ws_handler(
    State(state): State<ServerState>,
    Query(query): Query<WsQuery>,
    headers: axum::http::HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    // SEC-04: the token already gates the handshake; the Origin allowlist is
    // defense in depth against a stolen-token client that is not the
    // extension itself (regular web origins, sandboxed frames, raw sockets).
    if !origin_allowed(
        headers
            .get(axum::http::header::ORIGIN)
            .and_then(|value| value.to_str().ok()),
    ) {
        return axum::http::StatusCode::FORBIDDEN.into_response();
    }
    if query.token != state.realtime.token {
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    }
    ws.max_message_size(64 * 1024)
        .on_upgrade(move |socket| handle_socket(state, socket))
}

async fn handle_socket(state: ServerState, socket: WebSocket) {
    *state.realtime.connected_clients.lock().await += 1;
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.realtime.tx.subscribe();

    if let Some(bootstrap) = state.realtime.bootstrap().await {
        let _ = send_json(&mut sender, &BrowserRealtimeEvent::Ready(bootstrap)).await;
    }
    if let Ok(tasks) = current_tasks(&state.app).await {
        let _ = send_json(&mut sender, &BrowserRealtimeEvent::TasksSnapshot { tasks }).await;
    }

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        if send_json(&mut sender, &event).await.is_err() {
                            break;
                        }
                    }
                    // A-6: On Lagged (slow client fell behind > 256 events),
                    // resync with a fresh TasksSnapshot instead of falling
                    // through to `else => break` which silently dropped the
                    // connection.
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(missed = n, "realtime client lagged, resyncing with snapshot");
                        if let Ok(tasks) = current_tasks(&state.app).await {
                            if send_json(&mut sender, &BrowserRealtimeEvent::TasksSnapshot { tasks }).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            Some(Ok(message)) = receiver.next() => {
                if let Message::Text(text) = message {
                    let response = handle_client_message(&state, &text).await;
                    if sender.send(Message::Text(response.into())).await.is_err() {
                        break;
                    }
                }
            }
            else => break,
        }
    }
    let mut clients = state.realtime.connected_clients.lock().await;
    *clients = clients.saturating_sub(1);
}

async fn handle_client_message(server: &ServerState, raw: &str) -> String {
    let app = &server.app;
    let parsed = serde_json::from_str::<BrowserClientEnvelope>(raw);
    let (id, result) = match parsed {
        Ok(message) if message.message_type == "createDownload" => {
            // SEC-04: structured over-quota rejection — a stolen token must
            // not be able to exhaust disk or amplify traffic without bound.
            if !server.realtime.try_acquire_create_download() {
                (
                    message.id,
                    Err(
                        "bridge_rate_limited: createDownload quota exceeded (10/minute); retry later."
                            .to_string(),
                    ),
                )
            } else {
                let id = message.id;
                let input = match message.payload {
                    Some(payload) => serde_json::from_value::<BrowserHandoffInput>(payload)
                        .map_err(|e| format!("Invalid create download payload: {e}")),
                    None => Err("Create download payload is required.".to_string()),
                };
                let state = app.state::<AppState>();
                (
                    id,
                    match input {
                        Ok(input) => commands::browser::create_browser_handoff_task_with_state(
                            app.clone(),
                            state.inner(),
                            input,
                        )
                        .await
                        .and_then(|value| serde_json::to_value(value).map_err(|e| e.to_string())),
                        Err(error) => Err(error),
                    },
                )
            }
        }
        Ok(message) if message.message_type == "getSettings" => {
            let state = app.state::<AppState>();
            (
                message.id,
                commands::browser::browser_capture_settings(&state.pool)
                    .await
                    .and_then(|value| serde_json::to_value(value).map_err(|e| e.to_string())),
            )
        }
        Ok(message) if message.message_type == "updateSettings" => {
            let state = app.state::<AppState>();
            let id = message.id;
            let input = match message.payload {
                Some(payload) => {
                    // S-1.1: Reject modifying sensitive fields via the WS bridge. These fields must be
                    // explicitly operated by the user in the main window UI (via Tauri commands), not by
                    // a local process holding the WS bootstrap token.
                    let conflicts = payload
                        .as_object()
                        .map(commands::browser::is_sensitive_settings_update)
                        .unwrap_or_default();
                    if !conflicts.is_empty() {
                        Err(format!(
                            "WS bridge cannot modify sensitive settings via updateSettings: {}. \
                             Use the main window UI to change these fields.",
                            conflicts.join(", ")
                        ))
                    } else {
                        serde_json::from_value::<BrowserCaptureSettingsInput>(payload)
                            .map_err(|e| format!("Invalid settings payload: {e}"))
                    }
                }
                None => Err("Settings payload is required.".to_string()),
            };
            let result = async {
                let input = input?;
                let current = commands::browser::browser_capture_settings(&state.pool).await?;
                let forward_headers_mode = input
                    .forward_headers_mode
                    .or_else(|| {
                        input.forward_headers.map(|enabled| {
                            if enabled {
                                crate::models::BrowserForwardHeadersMode::Enabled
                            } else {
                                crate::models::BrowserForwardHeadersMode::Disabled
                            }
                        })
                    })
                    .unwrap_or(current.forward_headers_mode);
                let next = commands::browser::enforce_browser_capture_settings_policy(
                    crate::models::BrowserCaptureSettings {
                        experimental_capture_enabled: input
                            .experimental_capture_enabled
                            .unwrap_or(current.experimental_capture_enabled),
                        auto_intercept: input.auto_intercept.unwrap_or(current.auto_intercept),
                        forward_headers: matches!(
                            forward_headers_mode,
                            crate::models::BrowserForwardHeadersMode::Enabled
                        ),
                        forward_headers_mode,
                        min_size_bytes: input.min_size_bytes.unwrap_or(current.min_size_bytes),
                        file_extensions: input.file_extensions.unwrap_or(current.file_extensions),
                        site_rules: input.site_rules.unwrap_or(current.site_rules),
                        allow_intranet_handoff: input
                            .allow_intranet_handoff
                            .unwrap_or(current.allow_intranet_handoff),
                    },
                );
                commands::browser::upsert_browser_capture_settings(&state.pool, &next).await?;
                if matches!(
                    next.forward_headers_mode,
                    crate::models::BrowserForwardHeadersMode::Disabled
                ) {
                    crate::db::clear_all_task_request_headers(&state.pool).await?;
                    state.request_headers.lock().await.clear();
                }
                serde_json::to_value(next).map_err(|e| e.to_string())
            }
            .await;
            (id, result)
        }
        Ok(message) if message.message_type == "ping" => {
            return serde_json::to_string(&BrowserServerResponse::<()>::Pong)
                .unwrap_or_else(|_| "{}".to_string());
        }
        Ok(message) => (
            message.id,
            Err(format!(
                "Unsupported browser realtime message: {}",
                message.message_type
            )),
        ),
        Err(error) => (
            None,
            Err(format!("Invalid browser realtime message: {error}")),
        ),
    };

    match result {
        Ok(value) => serde_json::to_string(&BrowserServerResponse::Result { id, value })
            .unwrap_or_else(|_| "{}".to_string()),
        Err(message) => serde_json::to_string(&BrowserServerResponse::<()>::Error { message })
            .unwrap_or_else(|_| "{}".to_string()),
    }
}

async fn current_tasks(app: &AppHandle) -> Result<Vec<Task>, String> {
    let state = app.state::<AppState>();
    let records = db::list_browser_realtime_task_records(&state.pool).await?;
    Ok(records.into_iter().map(Task::from).collect())
}

async fn send_json(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    value: &impl Serialize,
) -> Result<(), axum::Error> {
    let raw = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    sender.send(Message::Text(raw.into())).await
}

fn write_bootstrap_file(bootstrap: &BrowserBridgeBootstrap) -> Result<(), String> {
    let path = bootstrap_file_path();
    let raw = serde_json::to_string(bootstrap).map_err(|e| e.to_string())?;
    // Remove any stale bootstrap file from a previous run.
    // On Windows the file may be marked read-only, which blocks both
    // overwrite and deletion — clear the flag before removing.
    if path.exists() {
        #[cfg(windows)]
        {
            if let Ok(metadata) = std::fs::metadata(&path) {
                if metadata.permissions().readonly() {
                    let mut perms = metadata.permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    {
                        perms.set_readonly(false);
                    }
                    let _ = std::fs::set_permissions(&path, perms);
                }
            }
        }
        let _ = std::fs::remove_file(&path);
    }
    std::fs::write(&path, raw).map_err(|e| {
        format!(
            "Could not write browser bridge bootstrap file at {}: {e}",
            path.display()
        )
    })?;
    // Restrict the token file to read-only to limit tampering by other local processes.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400));
    }
    #[cfg(windows)]
    {
        if let Ok(mut perms) = std::fs::metadata(&path).map(|m| m.permissions()) {
            perms.set_readonly(true);
            let _ = std::fs::set_permissions(&path, perms);
        }
        // SEC-04: the readonly attribute is not an ACL — any process running
        // as a different user could still read the token. Replace the
        // inherited DACL with one granting only the current user.
        if let Err(error) = restrict_bootstrap_dacl(&path) {
            tracing::warn!(
                error = %error,
                "could not restrict browser bridge bootstrap file to the current user"
            );
        }
    }
    Ok(())
}

#[cfg(windows)]
fn restrict_bootstrap_dacl(path: &std::path::Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, ERROR_SUCCESS, GENERIC_ALL, HLOCAL};
    use windows::Win32::Security::Authorization::{
        SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W, GRANT_ACCESS,
        NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
    };
    use windows::Win32::Security::{
        ACL, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSID,
        SUB_CONTAINERS_AND_OBJECTS_INHERIT,
    };

    let sid_bytes = current_process_user_sid()?;
    // The trustee keeps referencing `sid_bytes`; both live to the end of this
    // function, so the pointer stays valid for every Win32 call below.
    let sid = PSID(sid_bytes.as_ptr() as *mut core::ffi::c_void);

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);

    let explicit = EXPLICIT_ACCESS_W {
        grfAccessPermissions: GENERIC_ALL.0,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            // TRUSTEE_IS_SID carries the SID in the `ptstrName` pointer slot.
            ptstrName: windows::core::PWSTR(sid.0.cast::<u16>()),
        },
    };
    let mut new_acl: *mut ACL = std::ptr::null_mut();
    let created = unsafe { SetEntriesInAclW(Some(&[explicit]), None, &mut new_acl) };
    if created != ERROR_SUCCESS || new_acl.is_null() {
        return Err(format!("SetEntriesInAclW failed: {}", created.0));
    }

    // PROTECTED_DACL drops every inherited ACE, so the resulting ACL contains
    // exactly the single current-user ACE built above.
    let applied = unsafe {
        SetNamedSecurityInfoW(
            PCWSTR::from_raw(wide.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(new_acl as *const ACL),
            None,
        )
    };
    unsafe { LocalFree(Some(HLOCAL(new_acl.cast()))) };
    if applied != ERROR_SUCCESS {
        return Err(format!("SetNamedSecurityInfoW failed: {}", applied.0));
    }
    Ok(())
}

/// SEC-04: the current process user's SID as raw self-relative bytes, used to
/// build the single-ACE DACL for the bootstrap file.
#[cfg(windows)]
fn current_process_user_sid() -> Result<Vec<u8>, String> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetLengthSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
        .map_err(|e| format!("OpenProcessToken failed: {e}"))?;
    let result = || -> Result<Vec<u8>, String> {
        let mut needed = 0_u32;
        // The first call is expected to fail and report the buffer size.
        let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut needed) };
        if needed == 0 {
            return Err("GetTokenInformation returned no buffer size.".to_string());
        }
        let mut buffer = vec![0_u8; needed as usize];
        unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                Some(buffer.as_mut_ptr().cast()),
                needed,
                &mut needed,
            )
        }
        .map_err(|e| format!("GetTokenInformation failed: {e}"))?;
        let token_user = unsafe { &*(buffer.as_ptr() as *const TOKEN_USER) };
        let sid = token_user.User.Sid;
        let length = unsafe { GetLengthSid(sid) } as usize;
        if length == 0 || length > buffer.len() {
            return Err("Current user SID has an invalid length.".to_string());
        }
        // Copy the SID out: it points into `buffer`, which must outlive only
        // this copy for the caller to keep using the returned bytes.
        Ok(unsafe { std::slice::from_raw_parts(sid.0 as *const u8, length).to_vec() })
    }();
    unsafe {
        let _ = CloseHandle(token);
    }
    result
}

pub fn read_bootstrap_file() -> Result<BrowserBridgeBootstrap, String> {
    let path = bootstrap_file_path();
    let raw = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "Could not read browser bridge bootstrap file at {}: {e}",
            path.display()
        )
    })?;
    serde_json::from_str(&raw).map_err(|e| format!("Invalid browser bridge bootstrap file: {e}"))
}

pub fn bootstrap_file_path() -> PathBuf {
    std::env::temp_dir().join(BOOTSTRAP_FILE_NAME)
}

#[cfg(test)]
mod bridge_guards_tests {
    use super::*;

    #[test]
    fn limiter_enforces_burst_then_refills_over_time() {
        let mut limiter = CreateDownloadRateLimiter::new(2, 120);
        assert!(limiter.try_acquire());
        assert!(limiter.try_acquire());
        assert!(!limiter.try_acquire(), "burst must be capped");
        // 120/minute = 2/second, so a 700 ms pause refills at least one token.
        std::thread::sleep(std::time::Duration::from_millis(700));
        assert!(limiter.try_acquire(), "tokens must refill over time");
    }

    #[test]
    fn only_extension_origins_may_connect() {
        assert!(origin_allowed(Some(
            "chrome-extension://abcdef/background.js"
        )));
        assert!(origin_allowed(Some("moz-extension://some-uuid/")));
        assert!(
            !origin_allowed(Some("https://evil.example")),
            "regular web origins must be rejected"
        );
        assert!(
            !origin_allowed(Some("null")),
            "sandboxed-frame origins must be rejected"
        );
        assert!(
            !origin_allowed(None),
            "non-browser clients must be rejected"
        );
    }
}

#[cfg(all(test, windows))]
mod windows_dacl_tests {
    use super::*;

    #[test]
    fn bootstrap_dacl_grants_only_the_current_user() {
        use std::os::windows::ffi::OsStrExt;

        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{LocalFree, ERROR_SUCCESS, HLOCAL};
        use windows::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
        use windows::Win32::Security::{
            GetAce, GetLengthSid, ACCESS_ALLOWED_ACE, DACL_SECURITY_INFORMATION,
            PSECURITY_DESCRIPTOR, PSID,
        };

        let dir = std::env::temp_dir().join(format!("vibe-dacl-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let path = dir.join("bootstrap.json");
        std::fs::write(&path, "{}").expect("write test file");

        restrict_bootstrap_dacl(&path).expect("restrict DACL");

        // Read the DACL back: exactly one ACE, granting GENERIC_ALL to the
        // current process user — that is the whole point of SEC-04.
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        wide.push(0);
        let mut dacl: *mut windows::Win32::Security::ACL = std::ptr::null_mut();
        let mut sd = PSECURITY_DESCRIPTOR::default();
        let status = unsafe {
            GetNamedSecurityInfoW(
                PCWSTR::from_raw(wide.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(&mut dacl),
                None,
                &mut sd,
            )
        };
        assert_eq!(status, ERROR_SUCCESS, "GetNamedSecurityInfoW failed");
        assert!(!dacl.is_null(), "DACL must exist after restriction");
        assert_eq!(
            unsafe { (*dacl).AceCount },
            1,
            "protected DACL must contain exactly one ACE"
        );

        let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
        unsafe { GetAce(dacl, 0, &mut ace) }.expect("GetAce failed");
        assert!(!ace.is_null());
        let allowed = unsafe { &*(ace as *const ACCESS_ALLOWED_ACE) };
        // GENERIC_ALL is normalized to FILE_ALL_ACCESS when the ACE is stored;
        // the generic bit itself never appears in an on-disk ACL.
        assert_eq!(
            allowed.Mask,
            windows::Win32::Storage::FileSystem::FILE_ALL_ACCESS.0,
            "ACE must grant full control"
        );

        // The ACE's SID starts right after the fixed header + mask fields.
        let sid_bytes = current_process_user_sid().expect("current user sid");
        let ace_sid_ptr = (&allowed.SidStart) as *const u32 as *const u8;
        let ace_sid_len =
            unsafe { GetLengthSid(PSID((&allowed.SidStart) as *const u32 as *mut _)) } as usize;
        let ace_sid = unsafe { std::slice::from_raw_parts(ace_sid_ptr, ace_sid_len) };
        assert_eq!(
            ace_sid,
            sid_bytes.as_slice(),
            "ACE must target the current user"
        );

        // The DACL lives inside the security descriptor allocation; only the
        // SD itself is freed.
        unsafe {
            LocalFree(Some(HLOCAL(sd.0.cast())));
        }
        std::fs::remove_dir_all(&dir).expect("cleanup test dir");
    }
}
