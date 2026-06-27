use tauri::State;

use crate::{
    db,
    models::{ClassificationRule, ClassificationRuleInput},
    AppState,
};

#[tauri::command]
#[specta::specta]
pub async fn list_classification_rules(
    state: State<'_, AppState>,
) -> Result<Vec<ClassificationRule>, String> {
    db::list_classification_rules(&state.pool).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_classification_rule(
    state: State<'_, AppState>,
    input: ClassificationRuleInput,
) -> Result<ClassificationRule, String> {
    db::create_classification_rule(&state.pool, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_classification_rule(
    state: State<'_, AppState>,
    id: String,
    input: ClassificationRuleInput,
) -> Result<ClassificationRule, String> {
    db::update_classification_rule(&state.pool, &id, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_classification_rule(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    db::delete_classification_rule(&state.pool, &id).await
}

#[tauri::command]
#[specta::specta]
pub async fn reorder_classification_rules(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<(), String> {
    db::reorder_classification_rules(&state.pool, ids).await
}
