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

pub async fn emit_task_updated_record(app: &AppHandle, pool: &SqlitePool, task: &TaskRecord) {
    let files = match db::list_task_file_records(pool, &task.id).await {
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
