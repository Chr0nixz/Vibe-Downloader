use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    browser_realtime::BrowserRealtimeEvent,
    clipboard::ClipboardLinkDetectedPayload,
    db,
    models::{
        CompletionActionRequestedPayload, Task, TaskProgressPayload, TaskRecord, TaskUpdatedPayload,
    },
};

pub const EVENT_TASK_PROGRESS: &str = "task-progress";
pub const EVENT_TASK_UPDATED: &str = "task-updated";
pub const EVENT_QUEUE_CHANGED: &str = "queue-changed";
pub const EVENT_SETTINGS_CHANGED: &str = "settings-changed";
pub const EVENT_BROWSER_HANDOFF_RECEIVED: &str = "browser-handoff-received";
pub const EVENT_BROWSER_HANDOFF_FAILED: &str = "browser-handoff-failed";
pub const EVENT_BROWSER_INTEGRATION_CHANGED: &str = "browser-integration-changed";
pub const EVENT_TRAY_NEW_DOWNLOAD_REQUESTED: &str = "tray-new-download-requested";
pub const EVENT_TRAY_SETTINGS_REQUESTED: &str = "tray-settings-requested";
pub const EVENT_CLIPBOARD_LINK_DETECTED: &str = "clipboard-link-detected";
pub const EVENT_COMPLETION_ACTION_REQUESTED: &str = "completion-action-requested";

const DEFAULT_PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(250);

pub struct TaskProgressEmitGate {
    min_interval: Duration,
    last_emit: Option<Instant>,
    pending: Option<TaskProgressPayload>,
}

impl Default for TaskProgressEmitGate {
    fn default() -> Self {
        Self::new(DEFAULT_PROGRESS_EMIT_INTERVAL)
    }
}

impl TaskProgressEmitGate {
    pub fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            last_emit: None,
            pending: None,
        }
    }

    pub fn emit_or_store(&mut self, app: &AppHandle, payload: TaskProgressPayload, force: bool) {
        let due = self
            .last_emit
            .is_none_or(|last_emit| last_emit.elapsed() >= self.min_interval);
        if force || due {
            emit_task_progress(app, &payload);
            self.last_emit = Some(Instant::now());
            self.pending = None;
        } else {
            self.pending = Some(payload);
        }
    }

    pub fn flush(&mut self, app: &AppHandle) {
        let Some(payload) = self.pending.take() else {
            return;
        };
        emit_task_progress(app, &payload);
        self.last_emit = Some(Instant::now());
    }
}

pub fn emit_task_progress(app: &AppHandle, payload: &TaskProgressPayload) {
    emit_payload(app, EVENT_TASK_PROGRESS, payload);
    if let Some(state) = app.try_state::<crate::AppState>() {
        state
            .browser_realtime
            .broadcast(BrowserRealtimeEvent::TaskProgress(payload.clone()));
    }
}

pub fn emit_task_updated(app: &AppHandle, task: &Task) {
    let payload = TaskUpdatedPayload { task: task.clone() };
    emit_payload(app, EVENT_TASK_UPDATED, &payload);
    if let Some(state) = app.try_state::<crate::AppState>() {
        state
            .browser_realtime
            .broadcast(BrowserRealtimeEvent::TaskUpdated(Box::new(task.clone())));
    }
}

/// E-7: Cache of task_id → last emitted files_version. When a task's
/// files_version hasn't changed since the last emit, we skip the
/// list_task_file_records query and emit with empty files — the frontend
/// store preserves existing files when an incoming task has no files
/// (see `mergedFiles` logic in upsertTask). This avoids a DB query on every
/// status/progress transition where files haven't structurally changed.
static FILES_VERSION_CACHE: OnceLock<Mutex<HashMap<String, i64>>> = OnceLock::new();

fn files_version_cache() -> &'static Mutex<HashMap<String, i64>> {
    FILES_VERSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub async fn emit_task_updated_record(app: &AppHandle, pool: &SqlitePool, task: &TaskRecord) {
    // E-7: short-circuit — if files_version hasn't changed since the last emit
    // for this task, skip the task_files query. The frontend keeps its existing
    // files when the incoming event has an empty files array.
    let cache_hit = {
        let cache = files_version_cache().lock().ok();
        cache.is_some_and(|guard| guard.get(&task.id) == Some(&task.files_version))
    };

    let files = if cache_hit {
        Vec::new()
    } else {
        let loaded = match db::list_task_file_records(pool, &task.id).await {
            Ok(files) => files.into_iter().map(Into::into).collect(),
            Err(error) => {
                tracing::warn!(
                    task_id = %task.id,
                    error = %error,
                    "failed to load task files for update event"
                );
                Vec::new()
            }
        };
        // Update cache with the current version so subsequent emits can skip.
        if let Ok(mut guard) = files_version_cache().lock() {
            guard.insert(task.id.clone(), task.files_version);
        }
        loaded
    };
    let mut task = Task::from(task.clone());
    task.files = files;
    emit_task_updated(app, &task);
}

pub fn emit_queue_changed(app: &AppHandle) {
    emit_empty(app, EVENT_QUEUE_CHANGED);
    if let Some(state) = app.try_state::<crate::AppState>() {
        state
            .browser_realtime
            .broadcast(BrowserRealtimeEvent::QueueChanged);
    }
}

pub fn emit_settings_changed(app: &AppHandle) {
    emit_empty(app, EVENT_SETTINGS_CHANGED);
}

pub fn emit_browser_handoff_received(app: &AppHandle) {
    emit_empty(app, EVENT_BROWSER_HANDOFF_RECEIVED);
}

pub fn emit_browser_handoff_failed(app: &AppHandle) {
    emit_empty(app, EVENT_BROWSER_HANDOFF_FAILED);
}

pub fn emit_browser_integration_changed(app: &AppHandle) {
    emit_empty(app, EVENT_BROWSER_INTEGRATION_CHANGED);
}

pub fn emit_tray_new_download_requested(app: &AppHandle) {
    emit_empty(app, EVENT_TRAY_NEW_DOWNLOAD_REQUESTED);
}

pub fn emit_tray_settings_requested(app: &AppHandle) {
    emit_empty(app, EVENT_TRAY_SETTINGS_REQUESTED);
}

pub fn emit_clipboard_link_detected(app: &AppHandle, payload: &ClipboardLinkDetectedPayload) {
    emit_payload(app, EVENT_CLIPBOARD_LINK_DETECTED, payload);
}

pub fn emit_completion_action_requested(
    app: &AppHandle,
    payload: &CompletionActionRequestedPayload,
) {
    emit_payload(app, EVENT_COMPLETION_ACTION_REQUESTED, payload);
}

fn emit_empty(app: &AppHandle, event: &str) {
    if let Err(error) = app.emit(event, ()) {
        tracing::warn!(event, error = %error, "failed to emit event");
    }
}

fn emit_payload<T: Clone + serde::Serialize>(app: &AppHandle, event: &str, payload: &T) {
    if let Err(error) = app.emit(event, payload) {
        tracing::warn!(event, error = %error, "failed to emit event");
    }
}
