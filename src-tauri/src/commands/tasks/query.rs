use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use crate::{
    db,
    models::{
        RequestDiagnostic, SegmentStatus, SegmentSummary, Task, TaskEvent, TaskRecord, TaskSegment,
        TaskStatsSnapshot, TaskStatus, TorrentRuntimeSnapshot,
    },
    AppState,
};

use super::{task_from_record_with_files, tasks_from_records_with_files};

const QUEUE_PRIORITY_STRIDE: i64 = 1_000_000_000_000;

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListSegmentsInput {
    pub task_id: String,
    pub page: Option<i32>,
    pub page_size: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CursorPageInput {
    pub task_id: String,
    pub cursor: Option<String>,
    pub page_size: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskEventsPageResult {
    pub items: Vec<TaskEvent>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskRequestsPageResult {
    pub items: Vec<RequestDiagnostic>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskSegmentsPageResult {
    pub items: Vec<TaskSegment>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HlsSegmentView {
    pub id: String,
    pub media_sequence: String,
    pub discontinuity_sequence: String,
    pub uri: String,
    pub duration_ms: String,
    pub status: SegmentStatus,
    pub retry_count: i32,
    pub last_error: Option<String>,
    pub downloaded_bytes: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HlsSegmentsPageResult {
    pub items: Vec<HlsSegmentView>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DashSegmentView {
    pub id: String,
    pub track_kind: String,
    pub segment_index: String,
    pub uri: String,
    pub status: SegmentStatus,
    pub retry_count: i32,
    pub last_error: Option<String>,
    pub downloaded_bytes: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DashSegmentsPageResult {
    pub items: Vec<DashSegmentView>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksInput {
    pub nav: Option<String>,
    pub search: Option<String>,
    pub sort_key: Option<String>,
    pub sort_direction: Option<String>,
    pub file_type: Option<String>,
    pub source: Option<String>,
    pub failure: Option<String>,
    pub resume: Option<String>,
    pub page: Option<i32>,
    pub page_size: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksResult {
    pub items: Vec<Task>,
    pub total: String,
    pub page: i32,
    pub page_size: i32,
}

#[tauri::command]
#[specta::specta]
pub async fn list_tasks(state: State<'_, AppState>) -> Result<Vec<Task>, String> {
    let records = db::list_task_records(&state.pool).await?;
    tasks_from_records_with_files(&state.pool, records).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_tasks_page(
    state: State<'_, AppState>,
    input: ListTasksInput,
) -> Result<ListTasksResult, String> {
    let query = db::TaskListQuery {
        nav: input.nav.unwrap_or_else(|| "all".to_string()),
        search: input.search.unwrap_or_default(),
        sort_key: input.sort_key.unwrap_or_else(|| "updated_at".to_string()),
        sort_direction: match input.sort_direction.as_deref() {
            Some("asc") => "asc".to_string(),
            _ => "desc".to_string(),
        },
        file_type: input.file_type.unwrap_or_else(|| "all".to_string()),
        source: input.source.unwrap_or_else(|| "all".to_string()),
        failure: input.failure.unwrap_or_else(|| "all".to_string()),
        resume: input.resume.unwrap_or_else(|| "all".to_string()),
        page: i64::from(input.page.unwrap_or(0)),
        page_size: i64::from(
            input
                .page_size
                .unwrap_or(i32::try_from(db::DEFAULT_TASK_PAGE_SIZE).unwrap_or(100)),
        ),
        cursor_value: None,
        cursor_id: None,
    };
    let page = db::list_task_records_page(&state.pool, &query).await?;
    let tasks = tasks_from_records_with_files(&state.pool, page.items).await?;
    Ok(ListTasksResult {
        items: tasks,
        total: page.total.to_string(),
        page: i32::try_from(page.page).unwrap_or(0),
        page_size: i32::try_from(page.page_size).unwrap_or(100),
    })
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksCursorInput {
    pub nav: Option<String>,
    pub search: Option<String>,
    pub sort_key: Option<String>,
    pub sort_direction: Option<String>,
    pub file_type: Option<String>,
    pub source: Option<String>,
    pub failure_category: Option<String>,
    pub resume: Option<String>,
    pub cursor: Option<String>,
    pub page_size: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskFilterOptions {
    pub sources: Vec<String>,
    pub failure_categories: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum QueueWaitReason {
    Ready,
    RetryDelay,
    ActiveLimit,
    ScheduleWindow,
    HostLimit,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QueueTaskDecision {
    pub task_id: String,
    pub reason: QueueWaitReason,
    pub host_used_slots: i32,
    pub host_limit: i32,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerHostSnapshot {
    pub source_key: String,
    pub used_slots: i32,
    pub limit: i32,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerSnapshot {
    pub generated_at: String,
    pub max_active_tasks: i32,
    pub active_task_count: i32,
    pub available_task_slots: i32,
    pub max_connections_per_host: i32,
    pub schedule_window_enabled: bool,
    pub schedule_window_active: bool,
    pub schedule_window_start: String,
    pub schedule_window_end: String,
    pub hosts: Vec<SchedulerHostSnapshot>,
    pub decisions: Vec<QueueTaskDecision>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksCursorResult {
    pub items: Vec<Task>,
    pub next_cursor: Option<String>,
    /// Lower bound for matching tasks. Cursor pagination deliberately avoids
    /// an extra COUNT query on the first-screen path.
    pub minimum_total: String,
    pub filter_options: TaskFilterOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskCursor {
    sort_value: String,
    id: String,
}

#[tauri::command]
#[specta::specta]
pub async fn list_tasks_cursor(
    state: State<'_, AppState>,
    input: ListTasksCursorInput,
) -> Result<ListTasksCursorResult, String> {
    let cursor = input
        .cursor
        .as_deref()
        .and_then(|value| serde_json::from_str::<TaskCursor>(value).ok());
    let sort_key = input.sort_key.unwrap_or_else(|| "updated_at".to_string());
    let query = db::TaskListQuery {
        nav: input.nav.unwrap_or_else(|| "all".to_string()),
        search: input.search.unwrap_or_default(),
        sort_key: sort_key.clone(),
        sort_direction: match input.sort_direction.as_deref() {
            Some("asc") => "asc".to_string(),
            _ => "desc".to_string(),
        },
        file_type: input.file_type.unwrap_or_else(|| "all".to_string()),
        source: input.source.unwrap_or_else(|| "all".to_string()),
        failure: input.failure_category.unwrap_or_else(|| "all".to_string()),
        resume: input.resume.unwrap_or_else(|| "all".to_string()),
        page: 0,
        page_size: i64::from(
            input
                .page_size
                .unwrap_or(i32::try_from(db::DEFAULT_TASK_PAGE_SIZE).unwrap_or(100)),
        ),
        cursor_value: cursor.as_ref().map(|cursor| cursor.sort_value.clone()),
        cursor_id: cursor.as_ref().map(|cursor| cursor.id.clone()),
    };
    let page = db::list_task_records_cursor(&state.pool, &query).await?;
    let next_cursor = page
        .has_more
        .then(|| {
            let record = page.items.last()?;
            serde_json::to_string(&TaskCursor {
                sort_value: task_cursor_value(record, &sort_key),
                id: record.id.clone(),
            })
            .unwrap_or_default()
            .into()
        })
        .flatten();
    let tasks = tasks_from_records_with_files(&state.pool, page.items).await?;
    let options = if cursor.is_some() {
        db::TaskFilterOptions {
            sources: Vec::new(),
            failure_categories: Vec::new(),
        }
    } else {
        db::task_filter_options(&state.pool).await?
    };
    Ok(ListTasksCursorResult {
        items: tasks,
        next_cursor,
        minimum_total: page.total.to_string(),
        filter_options: TaskFilterOptions {
            sources: options.sources,
            failure_categories: options.failure_categories,
        },
    })
}

fn task_cursor_value(task: &TaskRecord, sort_key: &str) -> String {
    match sort_key {
        "created_at" => task.created_at.clone(),
        "file_size" => task.total_size.to_string(),
        "progress" => {
            if task.total_size > 0 {
                ((task.downloaded_bytes as f64) / (task.total_size as f64)).to_string()
            } else {
                "0".to_string()
            }
        }
        "speed" => task.speed_bps.to_string(),
        "status" => status_rank(task.status).to_string(),
        "queue_order" => queue_order_value(task).to_string(),
        _ => task.updated_at.clone(),
    }
}

fn queue_order_value(task: &TaskRecord) -> i64 {
    let priority_rank = match task.priority {
        crate::models::TaskPriority::High => 0,
        crate::models::TaskPriority::Normal => 1,
        crate::models::TaskPriority::Low => 2,
    };
    // Queue positions are spaced by 1,000; this stride preserves scheduler
    // priority ordering while leaving ample room for large persisted queues.
    i64::from(priority_rank) * QUEUE_PRIORITY_STRIDE + task.queue_position
}

fn status_rank(status: TaskStatus) -> i32 {
    match status {
        TaskStatus::Downloading => 0,
        TaskStatus::Retrying => 1,
        TaskStatus::Queued => 2,
        TaskStatus::Paused => 3,
        TaskStatus::WaitingNetwork => 4,
        TaskStatus::NeedsAttention => 5,
        TaskStatus::Failed => 6,
        TaskStatus::Completed => 7,
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_task(state: State<'_, AppState>, id: String) -> Result<Option<Task>, String> {
    let Some(record) = db::get_task_record(&state.pool, &id).await? else {
        return Ok(None);
    };
    Ok(Some(
        task_from_record_with_files(&state.pool, record).await?,
    ))
}

/// E-1: Fetch multiple tasks by ID in a single query. Used by the frontend's
/// `onQueueChanged` handler to upsert only the changed tasks instead of
/// re-querying the entire first page when the backend emits
/// `QueueChangedPayload { changed_task_ids: Some(ids) }`.
#[tauri::command]
#[specta::specta]
pub async fn list_tasks_by_ids(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<Vec<Task>, String> {
    let records = db::list_task_records_by_ids(&state.pool, &ids).await?;
    tasks_from_records_with_files(&state.pool, records).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_task_stats(state: State<'_, AppState>) -> Result<TaskStatsSnapshot, String> {
    db::task_stats_snapshot(&state.pool).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_scheduler_snapshot(
    app: AppHandle,
    state: State<'_, AppState>,
    task_ids: Vec<String>,
) -> Result<SchedulerSnapshot, String> {
    let default_dir = crate::commands::settings::default_download_dir(&app).unwrap_or_default();
    let settings = db::get_settings(&state.pool, default_dir).await?;
    let (active_task_count, host_usage) = {
        let downloads = state.downloads.lock().await;
        let mut host_usage = HashMap::<String, usize>::new();
        for control in downloads.values() {
            *host_usage.entry(control.source_key.clone()).or_default() += control.connection_slots;
        }
        (downloads.len(), host_usage)
    };

    // The UI requests decisions only for its loaded cursor page. The cap keeps
    // a compromised WebView from creating an unbounded SQLite IN clause.
    let mut seen = HashSet::new();
    let scoped_task_ids = task_ids
        .into_iter()
        .filter(|id| seen.insert(id.clone()))
        .take(usize::try_from(db::MAX_TASK_PAGE_SIZE).unwrap_or(500))
        .collect::<Vec<_>>();
    let tasks = db::list_task_records_by_ids(&state.pool, &scoped_task_ids).await?;
    let schedule_window_active = db::local_time_window_active(
        &settings.schedule_download_window_start,
        &settings.schedule_download_window_end,
    );
    let now = chrono::Utc::now();
    let host_limit = settings.max_connections_per_host.max(1);
    let decisions = tasks
        .into_iter()
        .filter(|task| task.status == TaskStatus::Queued)
        .map(|task| {
            let host_used = host_usage.get(&task.source_key).copied().unwrap_or(0);
            let reason = queue_wait_reason(
                task.retry_after_at.as_deref(),
                now,
                active_task_count,
                settings.max_active_tasks,
                settings.schedule_download_window_enabled
                    && task.obey_schedule
                    && !schedule_window_active,
                host_used,
                usize::try_from(host_limit).unwrap_or(1),
            );
            QueueTaskDecision {
                task_id: task.id,
                reason,
                host_used_slots: i32::try_from(host_used).unwrap_or(i32::MAX),
                host_limit,
            }
        })
        .collect();
    let mut hosts = host_usage
        .into_iter()
        .map(|(source_key, used_slots)| SchedulerHostSnapshot {
            source_key,
            used_slots: i32::try_from(used_slots).unwrap_or(i32::MAX),
            limit: host_limit,
        })
        .collect::<Vec<_>>();
    hosts.sort_by(|left, right| left.source_key.cmp(&right.source_key));

    Ok(SchedulerSnapshot {
        generated_at: now.to_rfc3339(),
        max_active_tasks: settings.max_active_tasks,
        active_task_count: i32::try_from(active_task_count).unwrap_or(i32::MAX),
        available_task_slots: crate::scheduler::Scheduler::compute_available_slots(
            settings.max_active_tasks,
            active_task_count,
        ),
        max_connections_per_host: host_limit,
        schedule_window_enabled: settings.schedule_download_window_enabled,
        schedule_window_active,
        schedule_window_start: settings.schedule_download_window_start,
        schedule_window_end: settings.schedule_download_window_end,
        hosts,
        decisions,
    })
}

fn queue_wait_reason(
    retry_after_at: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
    active_task_count: usize,
    max_active_tasks: i32,
    schedule_blocked: bool,
    host_used_slots: usize,
    host_limit: usize,
) -> QueueWaitReason {
    let retry_delayed = retry_after_at
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .is_some_and(|value| value.with_timezone(&chrono::Utc) > now);
    if retry_delayed {
        return QueueWaitReason::RetryDelay;
    }
    if active_task_count >= usize::try_from(max_active_tasks.max(0)).unwrap_or(0) {
        return QueueWaitReason::ActiveLimit;
    }
    if schedule_blocked {
        return QueueWaitReason::ScheduleWindow;
    }
    if host_used_slots >= host_limit.max(1) {
        return QueueWaitReason::HostLimit;
    }
    QueueWaitReason::Ready
}

#[tauri::command]
#[specta::specta]
pub async fn list_segments(
    state: State<'_, AppState>,
    input: ListSegmentsInput,
) -> Result<Vec<TaskSegment>, String> {
    db::list_segment_records_paged(
        &state.pool,
        &input.task_id,
        i64::from(input.page.unwrap_or(0)),
        i64::from(input.page_size.unwrap_or(100)),
    )
    .await
    .map(|records| records.into_iter().map(TaskSegment::from).collect())
}

#[tauri::command]
#[specta::specta]
pub async fn list_segments_page(
    state: State<'_, AppState>,
    input: CursorPageInput,
) -> Result<TaskSegmentsPageResult, String> {
    let page_size = input.page_size.unwrap_or(100).clamp(1, 500);
    let cursor = input
        .cursor
        .as_deref()
        .and_then(|value| value.parse::<i64>().ok());
    let mut records =
        db::list_segment_records_cursor(&state.pool, &input.task_id, cursor, i64::from(page_size))
            .await?;
    let next_cursor = if records.len() > usize::try_from(page_size).unwrap_or(100) {
        records.pop().map(|segment| segment.range_start.to_string())
    } else {
        None
    };
    Ok(TaskSegmentsPageResult {
        items: records.into_iter().map(TaskSegment::from).collect(),
        next_cursor,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn list_hls_segments_page(
    state: State<'_, AppState>,
    input: CursorPageInput,
) -> Result<HlsSegmentsPageResult, String> {
    let page_size = input.page_size.unwrap_or(100).clamp(1, 500);
    let fetch_size = i64::from(page_size).saturating_add(1);
    let mut records = db::list_hls_segments_page(
        &state.pool,
        &input.task_id,
        input.cursor.as_deref(),
        fetch_size,
    )
    .await?;
    let next_cursor = if records.len() > usize::try_from(page_size).unwrap_or(100) {
        records.pop();
        records.last().map(db::hls_segment_cursor)
    } else {
        None
    };
    Ok(HlsSegmentsPageResult {
        items: records.into_iter().map(HlsSegmentView::from).collect(),
        next_cursor,
    })
}

impl From<db::HlsSegmentRecord> for HlsSegmentView {
    fn from(record: db::HlsSegmentRecord) -> Self {
        Self {
            id: record.id,
            media_sequence: record.media_sequence.to_string(),
            discontinuity_sequence: record.discontinuity_sequence.to_string(),
            uri: record.uri,
            duration_ms: record.duration_ms.to_string(),
            status: record.status,
            retry_count: record.retry_count,
            last_error: record.last_error,
            downloaded_bytes: record.downloaded_bytes.to_string(),
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn list_dash_segments_page(
    state: State<'_, AppState>,
    input: CursorPageInput,
) -> Result<DashSegmentsPageResult, String> {
    let page_size = input.page_size.unwrap_or(100).clamp(1, 500);
    let fetch_size = i64::from(page_size).saturating_add(1);
    let mut records = db::list_dash_segments_page(
        &state.pool,
        &input.task_id,
        input.cursor.as_deref(),
        fetch_size,
    )
    .await?;
    let next_cursor = if records.len() > usize::try_from(page_size).unwrap_or(100) {
        records.pop();
        records.last().map(db::dash_segment_cursor)
    } else {
        None
    };
    Ok(DashSegmentsPageResult {
        items: records.into_iter().map(DashSegmentView::from).collect(),
        next_cursor,
    })
}

impl From<db::DashSegmentRecord> for DashSegmentView {
    fn from(record: db::DashSegmentRecord) -> Self {
        Self {
            id: record.id,
            track_kind: record.track_kind,
            segment_index: record.segment_index.to_string(),
            uri: record.uri,
            status: record.status,
            retry_count: record.retry_count,
            last_error: record.last_error,
            downloaded_bytes: record.downloaded_bytes.to_string(),
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_segment_summary(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<SegmentSummary, String> {
    db::segment_summary(&state.pool, &task_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_torrent_runtime_snapshot(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<Option<TorrentRuntimeSnapshot>, String> {
    let mut snapshot = db::get_torrent_runtime_snapshot(&state.pool, &task_id)
        .await?
        .map(TorrentRuntimeSnapshot::from);
    if let Some(snapshot) = snapshot.as_mut() {
        let (ratio_limit, time_limit) = db::torrent_seeding_policy(&state.pool, &task_id).await?;
        snapshot.seed_ratio_limit = ratio_limit;
        snapshot.seed_time_limit_seconds = time_limit.map(|seconds| seconds.to_string());
    }
    Ok(snapshot)
}

#[tauri::command]
#[specta::specta]
pub async fn list_task_events_page(
    state: State<'_, AppState>,
    input: CursorPageInput,
) -> Result<TaskEventsPageResult, String> {
    let page_size = input.page_size.unwrap_or(100).clamp(1, 500);
    let cursor = input
        .cursor
        .as_deref()
        .and_then(|value| value.parse::<i64>().ok());
    let mut items =
        db::list_task_events_page(&state.pool, &input.task_id, cursor, i64::from(page_size))
            .await?;
    let next_cursor = if items.len() > usize::try_from(page_size).unwrap_or(100) {
        items.pop().map(|event| event.id)
    } else {
        None
    };
    Ok(TaskEventsPageResult { items, next_cursor })
}

#[tauri::command]
#[specta::specta]
pub async fn list_task_requests_page(
    state: State<'_, AppState>,
    input: CursorPageInput,
) -> Result<TaskRequestsPageResult, String> {
    let page_size = input.page_size.unwrap_or(100).clamp(1, 500);
    let cursor = input
        .cursor
        .as_deref()
        .and_then(|value| value.parse::<i64>().ok());
    let mut items = db::list_request_diagnostics_page(
        &state.pool,
        &input.task_id,
        cursor,
        i64::from(page_size),
    )
    .await?;
    let next_cursor = if items.len() > usize::try_from(page_size).unwrap_or(100) {
        items.pop().map(|request| request.id)
    } else {
        None
    };
    Ok(TaskRequestsPageResult { items, next_cursor })
}

#[cfg(test)]
mod tests {
    use super::{queue_wait_reason, QueueWaitReason};

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-07-13T12:00:00Z")
            .expect("valid fixture time")
            .with_timezone(&chrono::Utc)
    }

    #[test]
    fn queue_wait_reason_follows_scheduler_gate_order() {
        assert_eq!(
            queue_wait_reason(Some("2026-07-13T12:05:00Z"), now(), 2, 2, true, 8, 8,),
            QueueWaitReason::RetryDelay
        );
        assert_eq!(
            queue_wait_reason(None, now(), 2, 2, true, 8, 8),
            QueueWaitReason::ActiveLimit
        );
        assert_eq!(
            queue_wait_reason(None, now(), 1, 2, true, 8, 8),
            QueueWaitReason::ScheduleWindow
        );
        assert_eq!(
            queue_wait_reason(None, now(), 1, 2, false, 8, 8),
            QueueWaitReason::HostLimit
        );
        assert_eq!(
            queue_wait_reason(None, now(), 1, 2, false, 4, 8),
            QueueWaitReason::Ready
        );
    }
}
