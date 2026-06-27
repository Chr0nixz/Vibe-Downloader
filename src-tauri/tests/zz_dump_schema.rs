// Temporary diagnostic test: dumps the full schema (CREATE statements) after
// running the current migration set. Used to verify schema-baseline rewrite
// preserves the exact same schema. Delete this file after the baseline rewrite
// is verified.

use sqlx::Row;
use tauri_app_lib::db;

#[tokio::test]
async fn dump_full_schema() {
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-dump-{id}.sqlite"));
    let pool = db::connect(&path).await.expect("connect").pool;

    let rows = sqlx::query(
        "SELECT name, sql FROM sqlite_master \
         WHERE type IN ('table', 'index') \
         AND name NOT LIKE '_sqlx%' \
         AND name NOT LIKE 'sqlite_%' \
         ORDER BY type DESC, name ASC",
    )
    .fetch_all(&pool)
    .await
    .expect("query schema");

    let mut out = String::new();
    out.push_str("==== SCHEMA DUMP START ====\n");
    for row in rows {
        let name: String = row.get("name");
        let sql: Option<String> = row.get("sql");
        out.push_str(&format!("-- {name}\n"));
        out.push_str(&format!("{};\n\n", sql.unwrap_or_default()));
    }
    out.push_str("==== SCHEMA DUMP END ====\n");

    let dump_path = std::env::temp_dir().join("vibe_schema_new.txt");
    std::fs::write(&dump_path, &out).expect("write dump");
    println!("SCHEMA WRITTEN TO: {}", dump_path.display());

    pool.close().await;
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
}
