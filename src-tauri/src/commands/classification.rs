use std::path::PathBuf;

use tauri::{AppHandle, State};

use crate::{
    commands::settings::default_download_dir,
    db,
    models::{
        ClassificationRule, ClassificationRuleInput, PreviewClassificationInput,
        PreviewClassificationInputsUsed, PreviewClassificationResult,
    },
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

/// Preview which classification rule would apply without creating a task.
#[tauri::command]
#[specta::specta]
pub async fn preview_classification_match(
    app: AppHandle,
    state: State<'_, AppState>,
    input: PreviewClassificationInput,
) -> Result<PreviewClassificationResult, String> {
    let url = input.url.trim().to_string();
    if url.is_empty() {
        return Err("URL is required".to_string());
    }
    let file_name = input
        .file_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("")
        .to_string();
    let content_type = input
        .content_type
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    let inputs_used = PreviewClassificationInputsUsed {
        url: url.clone(),
        file_name: file_name.clone(),
        content_type: content_type.clone(),
    };

    let settings = db::get_settings(&state.pool, default_download_dir(&app)?).await?;
    let save_dir = PathBuf::from(&settings.default_save_dir);

    let manual = input
        .category_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    if let Some(category_key) = manual {
        let target_subdir = sanitize_category_key(&category_key);
        let effective_save_dir = target_subdir
            .as_ref()
            .map(|subdir| save_dir.join(subdir).display().to_string());
        return Ok(PreviewClassificationResult {
            matched: target_subdir.is_some(),
            manual_override: true,
            target_subdir,
            matched_rule: None,
            effective_save_dir,
            inputs_used,
        });
    }

    let rules = db::list_classification_rules(&state.pool).await?;
    let matched_rule =
        db::match_classification_rule(&url, &file_name, &content_type, &rules).cloned();
    let mut target_subdir = matched_rule.as_ref().map(|rule| rule.target_subdir.clone());
    if let Some(ref key) = target_subdir {
        if sanitize_category_key(key).is_none() {
            target_subdir = None;
        }
    }
    let effective_save_dir = target_subdir
        .as_ref()
        .map(|subdir| save_dir.join(subdir).display().to_string());

    Ok(PreviewClassificationResult {
        matched: target_subdir.is_some(),
        manual_override: false,
        matched_rule: if target_subdir.is_some() {
            matched_rule
        } else {
            None
        },
        target_subdir,
        effective_save_dir,
        inputs_used,
    })
}

fn sanitize_category_key(key: &str) -> Option<String> {
    let trimmed = key.trim();
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
    {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_category_key;
    use crate::db::match_classification_rule;
    use crate::models::{ClassificationMatchKind, ClassificationRule};

    fn rule(
        id: &str,
        enabled: bool,
        position: i32,
        match_kind: ClassificationMatchKind,
        pattern: &str,
        target: &str,
    ) -> ClassificationRule {
        ClassificationRule {
            id: id.to_string(),
            name: format!("rule-{id}"),
            enabled,
            position,
            match_kind,
            pattern: pattern.to_string(),
            target_subdir: target.to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn sanitize_rejects_path_traversal() {
        assert_eq!(sanitize_category_key("videos"), Some("videos".into()));
        assert_eq!(sanitize_category_key("../escape"), None);
        assert_eq!(sanitize_category_key("a/b"), None);
        assert_eq!(sanitize_category_key(""), None);
    }

    #[test]
    fn match_returns_full_rule() {
        let rules = vec![
            rule(
                "1",
                false,
                0,
                ClassificationMatchKind::Extension,
                "mp4",
                "disabled",
            ),
            rule(
                "2",
                true,
                1,
                ClassificationMatchKind::Extension,
                "mp4",
                "videos",
            ),
        ];
        let matched =
            match_classification_rule("https://example.com/file.mp4", "file.mp4", "", &rules);
        assert_eq!(matched.map(|r| r.id.as_str()), Some("2"));
        assert_eq!(matched.map(|r| r.target_subdir.as_str()), Some("videos"));
    }
}
