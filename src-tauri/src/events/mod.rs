use tauri::{AppHandle, Emitter, Manager};

use crate::models::TaskProgressPayload;

pub const EVENT_TASK_PROGRESS: &str = "task.progress";
pub const EVENT_QUEUE_CHANGED: &str = "queue.changed";

pub fn emit_task_progress(app: &AppHandle, payload: &TaskProgressPayload) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit(EVENT_TASK_PROGRESS, payload);
        return;
    }
    let _ = app.emit(EVENT_TASK_PROGRESS, payload);
}

pub fn emit_queue_changed(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit(EVENT_QUEUE_CHANGED, ());
        return;
    }
    let _ = app.emit(EVENT_QUEUE_CHANGED, ());
}
