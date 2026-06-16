use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::{
    db,
    models::{
        RequestDiagnostic, SegmentSummary, Task, TaskEvent, TaskRecord, TaskSegment,
        TaskStatsSnapshot, TaskStatus, TorrentRuntimeSnapshot,
    },
    AppState,
};

use super::{task_from_record_with_files, tasks_from_records_with_files};

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

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksCursorResult {
    pub items: Vec<Task>,
    pub next_cursor: Option<String>,
    pub total_estimate: String,
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
        total_estimate: page.total.to_string(),
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
        _ => task.updated_at.clone(),
    }
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

#[tauri::command]
#[specta::specta]
pub async fn get_task_stats(state: State<'_, AppState>) -> Result<TaskStatsSnapshot, String> {
    db::task_stats_snapshot(&state.pool).await
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
    db::get_torrent_runtime_snapshot(&state.pool, &task_id)
        .await
        .map(|snapshot| snapshot.map(TorrentRuntimeSnapshot::from))
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
