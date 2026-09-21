//! SEC-13: corrupted encrypted metadata must stay on Result paths at each DB entry point.

#![cfg(debug_assertions)]

use base64::Engine as _;
use tauri_app_lib::{
    db,
    models::{TaskProxyMode, TaskProxySettingsInput},
};

mod common;

#[tokio::test]
async fn malformed_nonce_is_reported_by_headers_credentials_and_proxy() {
    common::install_test_secret_key();
    let (_guard, pool) = common::test_pool("secure-metadata").await;
    for (id, protocol) in [
        ("header-task", "https"),
        ("credential-task", "sftp"),
        ("proxy-task", "sftp"),
    ] {
        let paths = common::TestPaths::new(id);
        let task = common::download_task(
            id,
            format!("{protocol}://example.com/{id}.bin"),
            protocol,
            &format!("{id}.bin"),
            1,
            &paths,
            false,
        );
        db::insert_task_record(&pool, &task).await.expect("task");
    }

    db::upsert_task_request_headers(
        &pool,
        "header-task",
        &[("cookie".to_string(), "session=ok".to_string())],
        None,
    )
    .await
    .expect("headers");
    db::upsert_task_credentials(
        &pool,
        "credential-task",
        "sftp",
        "alice",
        "secret",
        None,
        None,
    )
    .await
    .expect("credentials");
    db::upsert_task_proxy_settings(
        &pool,
        TaskProxySettingsInput {
            task_id: "proxy-task".to_string(),
            mode: TaskProxyMode::Custom,
            proxy_url: Some("socks5://127.0.0.1:1080".to_string()),
            proxy_username: Some("proxy-user".to_string()),
            proxy_password: Some("proxy-secret".to_string()),
            clear_proxy_password: None,
            no_proxy: None,
        },
    )
    .await
    .expect("proxy");

    use sqlx::Row;
    let base64 = base64::engine::general_purpose::STANDARD;
    for (table, field, id, code) in [
        (
            "task_request_headers",
            "headers_ciphertext",
            "header-task",
            "auth_headers_unavailable",
        ),
        (
            "task_credentials",
            "credentials_ciphertext",
            "credential-task",
            "task_credentials_decrypt_failed",
        ),
        (
            "task_proxy_settings",
            "proxy_password_ciphertext",
            "proxy-task",
            "proxy_secret_decrypt_failed",
        ),
    ] {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {field}, nonce FROM {table} WHERE task_id = ?"
        )))
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let ciphertext: String = row.get(field);
        let nonce: String = row.get("nonce");
        assert_eq!(base64.decode(&nonce).unwrap().len(), 12);
        assert!(resolve(&pool, id).await.is_ok(), "normal nonce must work");
        let mut cases: Vec<(String, String, &str)> = [0, 1, 11, 12, 13]
            .into_iter()
            .map(|len| (ciphertext.clone(), base64.encode(vec![0; len]), "nonce"))
            .collect();
        cases.extend([
            (base64.encode([1; 16]), nonce.clone(), field),
            ("!!invalid-ciphertext!!".into(), nonce.clone(), field),
            (ciphertext.clone(), "!!invalid-nonce!!".into(), "nonce"),
        ]);
        for (bad_ct, bad_nonce, bad_field) in cases {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE {table} SET {field} = ?, nonce = ? WHERE task_id = ?"
            )))
            .bind(&bad_ct)
            .bind(&bad_nonce)
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
            let pool_clone = pool.clone();
            let error = tokio::spawn(async move { resolve(&pool_clone, id).await })
                .await
                .expect("metadata must never panic")
                .unwrap_err();
            assert!(error.contains(code), "{error}");
            assert!(!error.contains("session=ok") && !error.contains("proxy-secret"));
            let structural = db::validate_backup_secrets(&pool).await;
            if base64.decode(&bad_nonce).is_ok_and(|n| n.len() == 12) && bad_ct == ciphertext {
                structural.expect("valid shape with a foreign nonce must not require decryption");
            } else {
                let error = structural.unwrap_err();
                assert!(
                    error.contains(table) && error.contains("record") && error.contains(bad_field),
                    "{error}"
                );
                assert!(!error.contains(&ciphertext));
            }
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE {table} SET {field} = ?, nonce = ? WHERE task_id = ?"
            )))
            .bind(&ciphertext)
            .bind(&nonce)
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
            assert!(
                resolve(&pool, id).await.is_ok(),
                "reconfigured credentials must recover"
            );
        }
    }
    // Structural validation accepts secrets that this machine cannot decrypt.
    sqlx::query("UPDATE task_credentials SET credentials_ciphertext = ?")
        .bind(base64.encode([1; 17]))
        .execute(&pool)
        .await
        .unwrap();
    db::validate_backup_secrets(&pool).await.unwrap();
    sqlx::query("UPDATE task_credentials SET nonce = ''")
        .execute(&pool)
        .await
        .unwrap();
    let paths = common::TestPaths::new("bad-backup-secret");
    sqlx::query("VACUUM INTO ?")
        .bind(paths.temp.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
    let schema = db::current_schema_version(&pool).await.unwrap();
    let error =
        db::materialize_and_verify_backup_db(&std::fs::read(&paths.temp).unwrap(), schema, schema)
            .await
            .unwrap_err();
    assert!(
        error.contains("task_credentials") && error.contains("nonce"),
        "{error}"
    );
    pool.close().await;
}

async fn resolve(pool: &sqlx::SqlitePool, id: &str) -> Result<(), String> {
    match id {
        "header-task" => db::resolve_task_request_headers(pool, id).await.map(|_| ()),
        "credential-task" => db::resolve_task_credentials(pool, id).await.map(|_| ()),
        "proxy-task" => db::resolve_task_proxy_config(pool, id, "sftp", &Default::default())
            .await
            .map(|_| ()),
        _ => unreachable!(),
    }
}
