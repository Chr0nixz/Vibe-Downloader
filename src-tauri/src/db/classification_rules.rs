use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::models::{
    task::now_iso, ClassificationMatchKind, ClassificationRule, ClassificationRuleInput,
};

pub async fn list_classification_rules(
    pool: &SqlitePool,
) -> Result<Vec<ClassificationRule>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, name, enabled, position, match_kind, pattern, target_subdir, created_at, updated_at
        FROM classification_rules
        ORDER BY position ASC, created_at ASC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows.into_iter().map(row_to_rule).collect())
}

pub async fn create_classification_rule(
    pool: &SqlitePool,
    input: ClassificationRuleInput,
) -> Result<ClassificationRule, String> {
    let id = Uuid::new_v4().to_string();
    let now = now_iso();
    let name = input
        .name
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    if name.is_empty() {
        return Err("Classification rule name is required.".to_string());
    }
    let match_kind = input
        .match_kind
        .unwrap_or(ClassificationMatchKind::Extension);
    let pattern = input
        .pattern
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    if pattern.is_empty() {
        return Err("Classification rule pattern is required.".to_string());
    }
    let target_subdir = input
        .target_subdir
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    if target_subdir.is_empty() {
        return Err("Classification rule target subdirectory is required.".to_string());
    }
    let enabled = input.enabled.unwrap_or(true);
    let position = match input.position {
        Some(value) => value,
        None => next_position(pool).await?,
    };

    sqlx::query(
        r#"
        INSERT INTO classification_rules (
            id, name, enabled, position, match_kind, pattern, target_subdir, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&name)
    .bind(enabled)
    .bind(position)
    .bind(match_kind.as_str())
    .bind(&pattern)
    .bind(&target_subdir)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    get_classification_rule(pool, &id)
        .await?
        .ok_or_else(|| "Classification rule not found after create.".to_string())
}

pub async fn update_classification_rule(
    pool: &SqlitePool,
    id: &str,
    input: ClassificationRuleInput,
) -> Result<ClassificationRule, String> {
    let current = get_classification_rule(pool, id)
        .await?
        .ok_or_else(|| "Classification rule not found.".to_string())?;
    let now = now_iso();
    let name = input
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or(current.name);
    if name.is_empty() {
        return Err("Classification rule name is required.".to_string());
    }
    let match_kind = input.match_kind.unwrap_or(current.match_kind);
    let pattern = input
        .pattern
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or(current.pattern);
    if pattern.is_empty() {
        return Err("Classification rule pattern is required.".to_string());
    }
    let target_subdir = input
        .target_subdir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or(current.target_subdir);
    if target_subdir.is_empty() {
        return Err("Classification rule target subdirectory is required.".to_string());
    }
    let enabled = input.enabled.unwrap_or(current.enabled);
    let position = input.position.unwrap_or(current.position);

    sqlx::query(
        r#"
        UPDATE classification_rules
        SET name = ?, enabled = ?, position = ?, match_kind = ?, pattern = ?, target_subdir = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&name)
    .bind(enabled)
    .bind(position)
    .bind(match_kind.as_str())
    .bind(&pattern)
    .bind(&target_subdir)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    get_classification_rule(pool, id)
        .await?
        .ok_or_else(|| "Classification rule not found after update.".to_string())
}

pub async fn delete_classification_rule(pool: &SqlitePool, id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM classification_rules WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn reorder_classification_rules(
    pool: &SqlitePool,
    ids: Vec<String>,
) -> Result<(), String> {
    let now = now_iso();
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    for (position, id) in ids.iter().enumerate() {
        sqlx::query("UPDATE classification_rules SET position = ?, updated_at = ? WHERE id = ?")
            .bind(position as i32)
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn get_classification_rule(
    pool: &SqlitePool,
    id: &str,
) -> Result<Option<ClassificationRule>, String> {
    let row = sqlx::query(
        r#"
        SELECT id, name, enabled, position, match_kind, pattern, target_subdir, created_at, updated_at
        FROM classification_rules
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(row.map(row_to_rule))
}

/// Return the first matching enabled rule. Rules must be sorted by position ascending.
pub fn match_classification_rule<'a>(
    url: &str,
    file_name: &str,
    content_type: &str,
    rules: &'a [ClassificationRule],
) -> Option<&'a ClassificationRule> {
    let url_lower = url.to_lowercase();
    let file_name_lower = file_name.to_lowercase();
    let content_type_lower = content_type.to_lowercase();
    for rule in rules {
        if !rule.enabled {
            continue;
        }
        let pattern_lower = rule.pattern.to_lowercase();
        let matched = match rule.match_kind {
            ClassificationMatchKind::Extension => {
                let pattern = if pattern_lower.starts_with('.') {
                    pattern_lower.clone()
                } else {
                    format!(".{pattern_lower}")
                };
                file_name_lower.ends_with(&pattern)
            }
            ClassificationMatchKind::Mime => content_type_lower.starts_with(&pattern_lower),
            ClassificationMatchKind::UrlContains => url_lower.contains(&pattern_lower),
        };
        if matched {
            return Some(rule);
        }
    }
    None
}

/// Apply classification rules to a download candidate and return the target subdirectory
/// of the first matching enabled rule. Rules are expected to be sorted by position ascending.
pub fn apply_classification_rules(
    url: &str,
    file_name: &str,
    content_type: &str,
    rules: &[ClassificationRule],
) -> Option<String> {
    match_classification_rule(url, file_name, content_type, rules)
        .map(|rule| rule.target_subdir.clone())
}

async fn next_position(pool: &SqlitePool) -> Result<i32, String> {
    let row =
        sqlx::query("SELECT COALESCE(MAX(position), -1) AS max_position FROM classification_rules")
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(row.get::<i64, _>("max_position") as i32 + 1)
}

fn row_to_rule(row: sqlx::sqlite::SqliteRow) -> ClassificationRule {
    let match_kind_str = row.get::<String, _>("match_kind");
    let match_kind = ClassificationMatchKind::from_str(&match_kind_str)
        .unwrap_or(ClassificationMatchKind::Extension);
    ClassificationRule {
        id: row.get("id"),
        name: row.get("name"),
        enabled: row.get::<i64, _>("enabled") != 0,
        position: row.get::<i64, _>("position") as i32,
        match_kind,
        pattern: row.get("pattern"),
        target_subdir: row.get("target_subdir"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ClassificationMatchKind;

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
    fn extension_match_case_insensitive() {
        let rules = vec![rule(
            "1",
            true,
            0,
            ClassificationMatchKind::Extension,
            "MP4",
            "videos",
        )];
        assert_eq!(
            apply_classification_rules("https://example.com/file.MP4", "file.MP4", "", &rules),
            Some("videos".to_string())
        );
    }

    #[test]
    fn extension_match_with_leading_dot() {
        let rules = vec![rule(
            "1",
            true,
            0,
            ClassificationMatchKind::Extension,
            ".mp4",
            "videos",
        )];
        assert_eq!(
            apply_classification_rules("https://example.com/file.mp4", "file.mp4", "", &rules),
            Some("videos".to_string())
        );
    }

    #[test]
    fn extension_no_match() {
        let rules = vec![rule(
            "1",
            true,
            0,
            ClassificationMatchKind::Extension,
            "mp4",
            "videos",
        )];
        assert_eq!(
            apply_classification_rules("https://example.com/file.pdf", "file.pdf", "", &rules),
            None
        );
    }

    #[test]
    fn mime_prefix_match() {
        let rules = vec![rule(
            "1",
            true,
            0,
            ClassificationMatchKind::Mime,
            "video/",
            "videos",
        )];
        assert_eq!(
            apply_classification_rules("https://example.com/stream", "stream", "video/mp4", &rules),
            Some("videos".to_string())
        );
    }

    #[test]
    fn url_contains_match() {
        let rules = vec![rule(
            "1",
            true,
            0,
            ClassificationMatchKind::UrlContains,
            "example.com/video",
            "videos",
        )];
        assert_eq!(
            apply_classification_rules(
                "https://EXAMPLE.com/video/file.mp4",
                "file.mp4",
                "",
                &rules
            ),
            Some("videos".to_string())
        );
    }

    #[test]
    fn disabled_rule_skipped() {
        let rules = vec![
            rule(
                "1",
                false,
                0,
                ClassificationMatchKind::Extension,
                "mp4",
                "videos",
            ),
            rule(
                "2",
                true,
                1,
                ClassificationMatchKind::Extension,
                "pdf",
                "docs",
            ),
        ];
        assert_eq!(
            apply_classification_rules("https://example.com/file.mp4", "file.mp4", "", &rules),
            None
        );
    }

    #[test]
    fn first_matching_rule_wins_by_position() {
        // Rules must be sorted by position ascending before passing to
        // apply_classification_rules (the function itself does not sort).
        let rules = vec![
            rule(
                "2",
                true,
                0,
                ClassificationMatchKind::UrlContains,
                "example.com",
                "site-early",
            ),
            rule(
                "1",
                true,
                1,
                ClassificationMatchKind::Extension,
                "mp4",
                "videos-late",
            ),
        ];
        assert_eq!(
            apply_classification_rules("https://example.com/file.mp4", "file.mp4", "", &rules),
            Some("site-early".to_string())
        );
    }

    #[test]
    fn empty_rules_returns_none() {
        assert_eq!(
            apply_classification_rules("https://example.com/file.mp4", "file.mp4", "", &[]),
            None
        );
    }
}
