//! SEC-01: scoped local-file read/write that replaces the `fs` plugin.
//!
//! The webview used to hold `fs:default` with `{ "path": "**" }`, which is a
//! whole-filesystem read (plus mkdir) primitive. Task-report export also called
//! `writeTextFile` / dialog `save` without those permissions, so the More menu
//! appeared to do nothing. These commands are the only filesystem entry points
//! the frontend needs:
//! - `read_local_text_file` for batch `.txt` lists and SSH private keys
//! - `write_export_file` for JSON/CSV reports the user picked in a save dialog

use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::fs;

use crate::models::{AppErrorPayload, RecoveryAction};

const BATCH_TEXT_MAX_BYTES: u64 = 1_048_576;
const SSH_KEY_MAX_BYTES: u64 = 64 * 1024;
const EXPORT_MAX_BYTES: usize = 16 * 1024 * 1024;

const SSH_KEY_NAMES: &[&str] = &["id_rsa", "id_ed25519", "id_ecdsa", "id_dsa", "identity"];
const SSH_KEY_EXTENSIONS: &[&str] = &["pem", "key"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LocalTextFileKind {
    BatchText,
    SshKey,
}

#[tauri::command]
#[specta::specta]
pub async fn read_local_text_file(path: String, kind: LocalTextFileKind) -> Result<String, String> {
    let path = Path::new(&path);
    validate_read_path(path, kind).map_err(command_error)?;

    let meta = fs::metadata(path).await.map_err(|error| {
        command_error(AppErrorPayload::new(
            "local_file_read_failed",
            format!("Could not read the local file: {error}"),
            true,
            vec![RecoveryAction::Retry.as_str()],
        ))
    })?;
    let max_bytes = max_read_bytes(kind);
    if meta.len() > max_bytes {
        return Err(command_error(AppErrorPayload::new(
            "local_file_too_large",
            format!("The file is larger than {max_bytes} bytes."),
            false,
            vec![],
        )));
    }

    let bytes = fs::read(path).await.map_err(|error| {
        command_error(AppErrorPayload::new(
            "local_file_read_failed",
            format!("Could not read the local file: {error}"),
            true,
            vec![RecoveryAction::Retry.as_str()],
        ))
    })?;
    String::from_utf8(bytes).map_err(|_| {
        command_error(AppErrorPayload::new(
            "local_file_read_failed",
            "The file is not valid UTF-8 text.",
            false,
            vec![],
        ))
    })
}

#[tauri::command]
#[specta::specta]
pub async fn write_export_file(path: String, contents: String) -> Result<(), String> {
    if contents.len() > EXPORT_MAX_BYTES {
        return Err(command_error(AppErrorPayload::new(
            "export_too_large",
            format!("Export contents exceed {EXPORT_MAX_BYTES} bytes."),
            false,
            vec![],
        )));
    }
    let path = Path::new(&path);
    validate_export_path(path).map_err(command_error)?;
    fs::write(path, contents.as_bytes()).await.map_err(|error| {
        command_error(AppErrorPayload::new(
            "export_write_failed",
            format!("Could not write the export file: {error}"),
            true,
            vec![RecoveryAction::ChooseAnotherFolder.as_str()],
        ))
    })
}

fn max_read_bytes(kind: LocalTextFileKind) -> u64 {
    match kind {
        LocalTextFileKind::BatchText => BATCH_TEXT_MAX_BYTES,
        LocalTextFileKind::SshKey => SSH_KEY_MAX_BYTES,
    }
}

fn validate_read_path(path: &Path, kind: LocalTextFileKind) -> Result<(), AppErrorPayload> {
    require_safe_user_path(path, "local_file_denied")?;
    if !kind_allows_path(path, kind) {
        return Err(denied(
            "local_file_denied",
            "This local file type is not allowed.",
        ));
    }
    Ok(())
}

fn validate_export_path(path: &Path) -> Result<(), AppErrorPayload> {
    require_safe_user_path(path, "export_path_denied")?;
    match extension_lower(path).as_deref() {
        Some("json" | "csv") => Ok(()),
        _ => Err(denied(
            "export_path_denied",
            "Task reports can only be written as .json or .csv.",
        )),
    }
}

/// Absolute paths only; reject `..` so a picked path cannot walk sideways.
fn require_safe_user_path(path: &Path, code: &str) -> Result<(), AppErrorPayload> {
    if !path.is_absolute() {
        return Err(denied(code, "The path must be absolute."));
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(denied(code, "The path must not contain '..'."));
    }
    Ok(())
}

fn kind_allows_path(path: &Path, kind: LocalTextFileKind) -> bool {
    match kind {
        LocalTextFileKind::BatchText => extension_lower(path).as_deref() == Some("txt"),
        LocalTextFileKind::SshKey => ssh_key_name_allowed(path),
    }
}

fn ssh_key_name_allowed(path: &Path) -> bool {
    let Some(name) = file_name_lower(path) else {
        return false;
    };
    if SSH_KEY_NAMES.contains(&name.as_str()) {
        return true;
    }
    extension_lower(path).is_some_and(|ext| SSH_KEY_EXTENSIONS.contains(&ext.as_str()))
}

fn file_name_lower(path: &Path) -> Option<String> {
    path.file_name()?.to_str().map(str::to_ascii_lowercase)
}

fn extension_lower(path: &Path) -> Option<String> {
    path.extension()?.to_str().map(str::to_ascii_lowercase)
}

fn denied(code: &str, message: &str) -> AppErrorPayload {
    AppErrorPayload::new(code, message, false, vec![])
}

fn command_error(payload: AppErrorPayload) -> String {
    payload.command_error()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn abs(parts: &[&str]) -> PathBuf {
        let mut path = if cfg!(windows) {
            PathBuf::from(r"C:\")
        } else {
            PathBuf::from("/")
        };
        for part in parts {
            path.push(part);
        }
        path
    }

    fn error_code(payload: AppErrorPayload) -> String {
        payload.code
    }

    #[test]
    fn batch_text_accepts_txt_and_rejects_other_extensions() {
        assert!(validate_read_path(&abs(&["urls.txt"]), LocalTextFileKind::BatchText).is_ok());
        assert_eq!(
            error_code(
                validate_read_path(&abs(&["urls.csv"]), LocalTextFileKind::BatchText).unwrap_err()
            ),
            "local_file_denied"
        );
        assert_eq!(
            error_code(
                validate_read_path(
                    &abs(&["Windows", "System32", "config", "SAM"]),
                    LocalTextFileKind::BatchText
                )
                .unwrap_err()
            ),
            "local_file_denied"
        );
    }

    #[test]
    fn ssh_key_accepts_known_names_and_key_extensions() {
        assert!(validate_read_path(
            &abs(&["Users", "me", ".ssh", "id_rsa"]),
            LocalTextFileKind::SshKey
        )
        .is_ok());
        assert!(validate_read_path(
            &abs(&["Users", "me", ".ssh", "id_ed25519"]),
            LocalTextFileKind::SshKey
        )
        .is_ok());
        assert!(validate_read_path(
            &abs(&["home", "me", ".ssh", "server.pem"]),
            LocalTextFileKind::SshKey
        )
        .is_ok());
        assert!(validate_read_path(
            &abs(&["home", "me", ".ssh", "server.key"]),
            LocalTextFileKind::SshKey
        )
        .is_ok());
        assert_eq!(
            error_code(
                validate_read_path(&abs(&["etc", "passwd"]), LocalTextFileKind::SshKey)
                    .unwrap_err()
            ),
            "local_file_denied"
        );
        assert_eq!(
            error_code(
                validate_read_path(
                    &abs(&["Users", "me", ".ssh", "id_rsa.pub"]),
                    LocalTextFileKind::SshKey
                )
                .unwrap_err()
            ),
            "local_file_denied"
        );
    }

    #[test]
    fn rejects_relative_paths_and_parent_dir_components() {
        assert_eq!(
            error_code(
                validate_read_path(Path::new("urls.txt"), LocalTextFileKind::BatchText)
                    .unwrap_err()
            ),
            "local_file_denied"
        );
        assert_eq!(
            error_code(
                validate_read_path(
                    &abs(&["Users", "me", "..", "..", "Windows", "win.ini"]),
                    LocalTextFileKind::BatchText,
                )
                .unwrap_err()
            ),
            "local_file_denied"
        );
        assert_eq!(
            error_code(validate_export_path(Path::new("vibe-tasks.json")).unwrap_err()),
            "export_path_denied"
        );
    }

    #[test]
    fn export_accepts_json_and_csv_only() {
        assert!(validate_export_path(&abs(&["Users", "me", "Desktop", "vibe-tasks.json"])).is_ok());
        assert!(validate_export_path(&abs(&["Users", "me", "Desktop", "vibe-tasks.CSV"])).is_ok());
        assert_eq!(
            error_code(
                validate_export_path(&abs(&["Users", "me", "Desktop", "vibe-tasks.exe"]))
                    .unwrap_err()
            ),
            "export_path_denied"
        );
        assert_eq!(
            error_code(validate_export_path(&abs(&["Users", "me", ".ssh", "id_rsa"])).unwrap_err()),
            "export_path_denied"
        );
    }

    #[tokio::test]
    async fn write_export_file_round_trip_and_rejects_wrong_extension() {
        let path = std::env::temp_dir().join(format!("vibe-export-{}.json", uuid::Uuid::new_v4()));
        write_export_file(
            path.to_string_lossy().into_owned(),
            "[{\"id\":\"1\"}]".into(),
        )
        .await
        .expect("write json export");
        let body = std::fs::read_to_string(&path).expect("read export");
        assert_eq!(body, "[{\"id\":\"1\"}]");
        let _ = std::fs::remove_file(&path);

        let denied = write_export_file(
            std::env::temp_dir()
                .join("vibe-export.exe")
                .to_string_lossy()
                .into_owned(),
            "[]".into(),
        )
        .await
        .expect_err("exe export");
        assert!(denied.contains("export_path_denied"), "{denied}");
    }

    #[tokio::test]
    async fn read_local_text_file_enforces_kind_and_size() {
        let dir = std::env::temp_dir();
        let txt = dir.join(format!("vibe-batch-{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&txt, "https://example.com/a.zip\n").expect("write txt");
        let body = read_local_text_file(
            txt.to_string_lossy().into_owned(),
            LocalTextFileKind::BatchText,
        )
        .await
        .expect("read txt");
        assert!(body.contains("example.com"));
        let ssh_denied = read_local_text_file(
            txt.to_string_lossy().into_owned(),
            LocalTextFileKind::SshKey,
        )
        .await
        .expect_err("txt is not an ssh key");
        assert!(ssh_denied.contains("local_file_denied"), "{ssh_denied}");
        let _ = std::fs::remove_file(&txt);

        let key = dir.join(format!("vibe-key-{}.pem", uuid::Uuid::new_v4()));
        std::fs::write(&key, "-----BEGIN OPENSSH PRIVATE KEY-----\n").expect("write key");
        let pem = read_local_text_file(
            key.to_string_lossy().into_owned(),
            LocalTextFileKind::SshKey,
        )
        .await
        .expect("read pem");
        assert!(pem.contains("BEGIN OPENSSH"));
        let _ = std::fs::remove_file(&key);
    }

    #[test]
    fn capabilities_no_longer_grant_unscoped_fs() {
        let raw = include_str!("../../capabilities/default.json");
        assert!(
            !raw.contains("fs:default"),
            "fs plugin must not remain in capabilities"
        );
        assert!(
            !raw.contains("\"**\""),
            "capabilities must not contain an unrooted glob"
        );
        assert!(
            raw.contains("dialog:allow-save"),
            "save dialog is required for export and backup"
        );
    }
}
