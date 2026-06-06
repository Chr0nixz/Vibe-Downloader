use tauri::{AppHandle, Emitter, Manager};

use crate::models::TaskProgressPayload;

pub const EVENT_TASK_PROGRESS: &str = "task.progress";
pub const EVENT_QUEUE_CHANGED: &str = "queue.changed";
pub const EVENT_SETTINGS_CHANGED: &str = "settings.changed";
pub const EVENT_BROWSER_HANDOFF_RECEIVED: &str = "browser.handoff.received";
pub const EVENT_BROWSER_HANDOFF_FAILED: &str = "browser.handoff.failed";
pub const EVENT_BROWSER_INTEGRATION_CHANGED: &str = "browser.integration.changed";

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

pub fn emit_settings_changed(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit(EVENT_SETTINGS_CHANGED, ());
        return;
    }
    let _ = app.emit(EVENT_SETTINGS_CHANGED, ());
}

pub fn emit_browser_handoff_received(app: &AppHandle) {
    emit_browser_event(app, EVENT_BROWSER_HANDOFF_RECEIVED);
}

pub fn emit_browser_handoff_failed(app: &AppHandle) {
    emit_browser_event(app, EVENT_BROWSER_HANDOFF_FAILED);
}

pub fn emit_browser_integration_changed(app: &AppHandle) {
    emit_browser_event(app, EVENT_BROWSER_INTEGRATION_CHANGED);
}

fn emit_browser_event(app: &AppHandle, event: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit(event, ());
        return;
    }
    let _ = app.emit(event, ());
}
