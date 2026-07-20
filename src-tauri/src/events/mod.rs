use std::collections::{HashMap, VecDeque};
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
pub const EVENT_PROBE_PHASE: &str = "probe-phase";

const DEFAULT_PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(250);

pub struct TaskProgressEmitGate {
    min_interval: Duration,
    last_emit: Option<Instant>,
    pending: Option<TaskProgressPayload>,
}

/// Resolves the production Tauri handle used by download events. Download
/// engines accept an optional handle so their core I/O can run headlessly in
/// integration tests without weakening event delivery in the scheduler.
pub trait DownloadEventTarget {
    fn app_handle(&self) -> Option<&AppHandle>;
}

impl DownloadEventTarget for AppHandle {
    fn app_handle(&self) -> Option<&AppHandle> {
        Some(self)
    }
}

impl DownloadEventTarget for Option<AppHandle> {
    fn app_handle(&self) -> Option<&AppHandle> {
        self.as_ref()
    }
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

    pub fn emit_or_store<T: DownloadEventTarget + ?Sized>(
        &mut self,
        app: &T,
        payload: TaskProgressPayload,
        force: bool,
    ) {
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

    pub fn flush<T: DownloadEventTarget + ?Sized>(&mut self, app: &T) {
        let Some(payload) = self.pending.take() else {
            return;
        };
        emit_task_progress(app, &payload);
        self.last_emit = Some(Instant::now());
    }
}

/// E-4: Throttles per-segment DB writes (task progress + segment runtime
/// progress) to the same cadence as IPC event emission. The last `force`d
/// write and the final state are always flushed by the caller via `flush`.
#[derive(Debug)]
pub struct DbWriteGate {
    min_interval: Duration,
    last_write: Option<Instant>,
    pending: bool,
}

impl Default for DbWriteGate {
    fn default() -> Self {
        Self::new(DEFAULT_PROGRESS_EMIT_INTERVAL)
    }
}

impl DbWriteGate {
    pub fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            last_write: None,
            pending: false,
        }
    }

    /// Returns `true` if a DB write should be performed now, `false` if it
    /// should be skipped (the caller will retry on the next segment). A
    /// `force=true` always returns `true` and resets the timer.
    pub fn should_write(&mut self, force: bool) -> bool {
        if force {
            self.last_write = Some(Instant::now());
            self.pending = false;
            return true;
        }
        let due = self
            .last_write
            .is_none_or(|last_write| last_write.elapsed() >= self.min_interval);
        if due {
            self.last_write = Some(Instant::now());
            self.pending = false;
            true
        } else {
            self.pending = true;
            false
        }
    }

    /// Returns `true` if there is a pending write that was skipped since the
    /// last `should_write` returned `true`. Callers check this at the end of
    /// the download loop (or on pause/cancel) to flush the final state.
    pub fn flush_pending(&mut self) -> bool {
        if self.pending {
            self.pending = false;
            self.last_write = Some(Instant::now());
            true
        } else {
            false
        }
    }
}

pub fn emit_task_progress<T: DownloadEventTarget + ?Sized>(
    target: &T,
    payload: &TaskProgressPayload,
) {
    let Some(app) = target.app_handle() else {
        return;
    };
    emit_payload(app, EVENT_TASK_PROGRESS, payload);
    if let Some(state) = app.try_state::<crate::AppState>() {
        state
            .browser_realtime
            .broadcast(BrowserRealtimeEvent::TaskProgress(payload.clone()));
    }
}

pub fn emit_task_updated<T: DownloadEventTarget + ?Sized>(target: &T, task: &Task) {
    let Some(app) = target.app_handle() else {
        return;
    };
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
///
/// PERF-05: bounded so completed history cannot grow the process map without limit.
const FILES_VERSION_CACHE_MAX_ENTRIES: usize = 4096;

struct FilesVersionCache {
    map: HashMap<String, i64>,
    order: VecDeque<String>,
}

impl FilesVersionCache {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&self, task_id: &str) -> Option<i64> {
        self.map.get(task_id).copied()
    }

    fn insert(&mut self, task_id: String, version: i64) {
        if let Some(slot) = self.map.get_mut(&task_id) {
            *slot = version;
            return;
        }
        while self.map.len() >= FILES_VERSION_CACHE_MAX_ENTRIES {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            } else {
                break;
            }
        }
        self.order.push_back(task_id.clone());
        self.map.insert(task_id, version);
    }

    fn remove(&mut self, task_id: &str) {
        if self.map.remove(task_id).is_some() {
            self.order.retain(|id| id != task_id);
        }
    }

    fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.map.len()
    }
}

static FILES_VERSION_CACHE: OnceLock<Mutex<FilesVersionCache>> = OnceLock::new();

fn files_version_cache() -> &'static Mutex<FilesVersionCache> {
    FILES_VERSION_CACHE.get_or_init(|| Mutex::new(FilesVersionCache::new()))
}

pub fn evict_task_files_version(task_id: &str) {
    if let Ok(mut cache) = files_version_cache().lock() {
        cache.remove(task_id);
    }
}

pub fn evict_task_files_versions<'a>(task_ids: impl IntoIterator<Item = &'a str>) {
    if let Ok(mut cache) = files_version_cache().lock() {
        for task_id in task_ids {
            cache.remove(task_id);
        }
    }
}

pub fn clear_task_files_version_cache() {
    if let Ok(mut cache) = files_version_cache().lock() {
        cache.clear();
    }
}
pub async fn emit_task_updated_record<T: DownloadEventTarget + ?Sized>(
    app: &T,
    pool: &SqlitePool,
    task: &TaskRecord,
) {
    if app.app_handle().is_none() {
        return;
    }
    // E-7: short-circuit — if files_version hasn't changed since the last emit
    // for this task, skip the task_files query. The frontend keeps its existing
    // files when the incoming event has an empty files array.
    let cache_hit = {
        let cache = files_version_cache().lock().ok();
        cache.is_some_and(|guard| guard.get(&task.id) == Some(task.files_version))
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
    emit_queue_changed_with_ids(app, None);
}

/// E-1: Emit a queue-changed event carrying the IDs of the tasks that changed.
/// The frontend uses this to fetch only the changed tasks via `list_tasks_by_ids`
/// instead of re-querying the entire first page. Pass `None` to signal a full
/// refresh (batch operations, settings changes that affect all tasks).
pub fn emit_queue_changed_with_ids<T: DownloadEventTarget + ?Sized>(
    target: &T,
    ids: Option<Vec<String>>,
) {
    let Some(app) = target.app_handle() else {
        return;
    };
    let _ = app.emit(
        EVENT_QUEUE_CHANGED,
        QueueChangedPayload {
            changed_task_ids: ids,
        },
    );
    if let Some(state) = app.try_state::<crate::AppState>() {
        state
            .browser_realtime
            .broadcast(BrowserRealtimeEvent::QueueChanged);
    }
}

/// E-1: Payload for the `queue-changed` event. `changed_task_ids` is `None`
/// when the change is batch/global (caller should do a full refresh), or
/// `Some(ids)` when a small set of tasks changed (caller can fetch just those
/// via `list_tasks_by_ids` for an incremental upsert).
#[derive(Clone, serde::Serialize, specta::Type)]
pub struct QueueChangedPayload {
    pub changed_task_ids: Option<Vec<String>>,
}

/// UX-6: Payload for the `probe-phase` event. Emitted by download engines
/// during `probe()` to give the NewDownloadDialog real, stage-aware feedback
/// instead of the old URL-regex static guess. `request_id` correlates events
/// to a specific probe invocation (passed by the frontend via `ProbeTaskInput`).
/// Not broadcast to `browser_realtime` — probe phases are ephemeral UI feedback.
#[derive(Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProbePhasePayload {
    pub request_id: String,
    pub kind: String,
    pub protocol: Option<String>,
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

/// UX-6: Emit a probe-phase event. Not broadcast to browser_realtime.
pub fn emit_probe_phase(app: &AppHandle, payload: &ProbePhasePayload) {
    emit_payload(app, EVENT_PROBE_PHASE, payload);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_file_version_cache_evicts_deleted_tasks() {
        clear_task_files_version_cache();
        {
            let mut cache = files_version_cache().lock().expect("cache lock");
            cache.insert("keep".to_string(), 1);
            cache.insert("remove-one".to_string(), 2);
            cache.insert("remove-two".to_string(), 3);
        }

        evict_task_files_version("remove-one");
        evict_task_files_versions(["remove-two"]);

        let cache = files_version_cache().lock().expect("cache lock");
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get("keep"), Some(1));
    }

    #[test]
    fn task_file_version_cache_evicts_oldest_when_full() {
        clear_task_files_version_cache();
        {
            let mut cache = files_version_cache().lock().expect("cache lock");
            for i in 0..FILES_VERSION_CACHE_MAX_ENTRIES {
                cache.insert(format!("t{i}"), i as i64);
            }
            assert_eq!(cache.len(), FILES_VERSION_CACHE_MAX_ENTRIES);
            cache.insert("overflow".to_string(), 99);
            assert_eq!(cache.len(), FILES_VERSION_CACHE_MAX_ENTRIES);
            assert_eq!(cache.get("t0"), None);
            assert_eq!(cache.get("overflow"), Some(99));
        }
        clear_task_files_version_cache();
    }
}
