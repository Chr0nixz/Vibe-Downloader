use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
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

#[derive(Debug)]
pub struct BrowserRealtimeState {
    token: String,
    bootstrap: Mutex<Option<BrowserBridgeBootstrap>>,
    connected_clients: Mutex<usize>,
    tx: broadcast::Sender<BrowserRealtimeEvent>,
}

impl BrowserRealtimeState {
    pub fn new() -> Arc<Self> {
        let (tx, _) = broadcast::channel(256);
        Arc::new(Self {
            token: Uuid::new_v4().to_string(),
            bootstrap: Mutex::new(None),
            connected_clients: Mutex::new(0),
            tx,
        })
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
        if let Err(error) = axum::serve(listener, router).await {
            tracing::error!(error = %error, "browser realtime bridge stopped");
        }
    });
    Ok(())
}

async fn ws_handler(
    State(state): State<ServerState>,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if query.token != state.realtime.token {
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(state, socket))
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
            Ok(event) = rx.recv() => {
                if send_json(&mut sender, &event).await.is_err() {
                    break;
                }
            }
            Some(Ok(message)) = receiver.next() => {
                if let Message::Text(text) = message {
                    let response = handle_client_message(&state.app, &text).await;
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

async fn handle_client_message(app: &AppHandle, raw: &str) -> String {
    let parsed = serde_json::from_str::<BrowserClientEnvelope>(raw);
    let (id, result) = match parsed {
        Ok(message) if message.message_type == "createDownload" => {
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
                Some(payload) => serde_json::from_value::<BrowserCaptureSettingsInput>(payload)
                    .map_err(|e| format!("Invalid settings payload: {e}")),
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
                        auto_intercept: input.auto_intercept.unwrap_or(current.auto_intercept),
                        forward_headers: matches!(
                            forward_headers_mode,
                            crate::models::BrowserForwardHeadersMode::Enabled
                        ),
                        forward_headers_mode,
                        min_size_bytes: input.min_size_bytes.unwrap_or(current.min_size_bytes),
                        file_extensions: input.file_extensions.unwrap_or(current.file_extensions),
                        site_rules: input.site_rules.unwrap_or(current.site_rules),
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
    let records = db::list_task_records(&state.pool).await?;
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
    std::fs::write(&path, raw).map_err(|e| {
        format!(
            "Could not write browser bridge bootstrap file at {}: {e}",
            path.display()
        )
    })
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
