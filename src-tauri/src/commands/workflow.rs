//! Explicit library application never bypasses ordinary task creation policy.
use crate::{
    db,
    models::{BulkTaskOptionsInput, BulkTaskOptionsResult, WorkflowEntry, WorkflowLibrary},
    AppState,
};
use tauri::{AppHandle, State};

#[tauri::command]
#[specta::specta]
pub async fn list_workflow_library(state: State<'_, AppState>) -> Result<WorkflowLibrary, String> {
    db::workflow::list(&state.pool)
        .await
        .map_err(|_| db::workflow::invalid_workflow())
}

#[tauri::command]
#[specta::specta]
pub async fn save_workflow_entry(
    state: State<'_, AppState>,
    entry: WorkflowEntry,
) -> Result<(), String> {
    db::workflow::save(&state.pool, &entry)
        .await
        .map_err(|_| db::workflow::invalid_workflow())
}

#[tauri::command]
#[specta::specta]
pub async fn delete_workflow_entry(state: State<'_, AppState>, id: String) -> Result<(), String> {
    db::workflow::delete(&state.pool, &id)
        .await
        .map_err(|_| db::workflow::invalid_workflow())
}

#[tauri::command]
#[specta::specta]
pub async fn bulk_update_task_options(
    app: AppHandle,
    state: State<'_, AppState>,
    input: BulkTaskOptionsInput,
) -> Result<Vec<BulkTaskOptionsResult>, String> {
    db::workflow::validate_patch(&input.patch)?;
    if input.ids.is_empty() || input.ids.len() > 100 {
        return Err(db::workflow::invalid_workflow());
    }
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for id in input.ids {
        if !seen.insert(id.clone()) {
            continue;
        }
        let outcome = {
            let _guard = state.task_runtime_locks.lock(&id).await;
            db::workflow::patch_task(&state.pool, &id, &input.patch).await
        };
        let (status, error) = match outcome {
            Ok(true) => ("applied", None),
            Ok(false) => ("skipped", None),
            Err(_) => ("failed", Some(db::workflow::invalid_workflow())),
        };
        results.push(BulkTaskOptionsResult {
            id,
            status: status.into(),
            error,
        });
    }
    let default_dir = super::settings::default_download_dir(&app).unwrap_or_default();
    if state
        .scheduler
        .refresh_speed_limit_policies(&state.pool, default_dir)
        .await
        .is_err()
    {
        for result in &mut results {
            if result.status == "applied" {
                result.status = "failed".into();
                result.error = Some(db::workflow::invalid_workflow());
            }
        }
    }
    for result in &results {
        if let Ok(Some(record)) = db::get_task_record(&state.pool, &result.id).await {
            crate::events::emit_task_updated_record(&app, &state.pool, &record).await;
        }
    }
    crate::events::emit_queue_changed_with_ids(
        &app,
        Some(results.iter().map(|r| r.id.clone()).collect()),
    );
    state
        .scheduler
        .clone()
        .dispatch(app, state.pool.clone())
        .await;
    Ok(results)
}
