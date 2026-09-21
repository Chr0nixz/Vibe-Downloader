//! Cross-protocol shared file operations: temp file preallocation, final path
//! resolution, completed path persistence.
//!
//! Originally in `download/http/file.rs`; moved to a protocol-neutral location
//! because it is reused by FTP/SFTP/DASH/HLS/Metalink and other HTTP-derived engines.

use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};

use sqlx::SqlitePool;
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
};
use uuid::Uuid;

use crate::{db, models::AppErrorPayload};

/// Publish only after file data is durable. The kernel rejects an occupied
/// destination in the same operation that installs the new name.
pub(crate) async fn finalize_download_file(
    temp_path: &Path,
    final_path: &Path,
) -> Result<PathBuf, String> {
    finalize_impl(temp_path, final_path, PublishOptions::default()).await
}

#[derive(Default)]
struct PublishOptions {
    #[cfg(debug_assertions)]
    force_staging: bool,
    #[cfg(debug_assertions)]
    fail_copy_after: Option<usize>,
    #[cfg(debug_assertions)]
    fail_sync: bool,
    #[cfg(debug_assertions)]
    before_publish: Option<Box<dyn FnOnce() + Send + Sync>>,
}

async fn finalize_impl(
    source: &Path,
    destination: &Path,
    mut options: PublishOptions,
) -> Result<PathBuf, String> {
    // E-11: fail before publication if data cannot be made durable; keeping the
    // original file lets recovery retry without accepting incomplete output.
    sync_source(source, &options).await.map_err(write_error)?;
    #[cfg(debug_assertions)]
    let force_staging = options.force_staging;
    #[cfg(not(debug_assertions))]
    let force_staging = false;
    let result = if force_staging {
        Err(std::io::Error::from(ErrorKind::CrossesDevices))
    } else {
        before_publish(&mut options);
        publish_noreplace(source, destination).await
    };
    match result {
        Ok(()) => Ok(destination.to_path_buf()),
        Err(error) if is_cross_device_error(&error) => {
            publish_across_volumes(source, destination, options).await?;
            Ok(destination.to_path_buf())
        }
        Err(error) => Err(publication_error(error, destination)),
    }
}

fn before_publish(options: &mut PublishOptions) {
    #[cfg(debug_assertions)]
    if let Some(hook) = options.before_publish.take() {
        hook();
    }
    #[cfg(not(debug_assertions))]
    let _ = options;
}

async fn sync_source(path: &Path, options: &PublishOptions) -> std::io::Result<()> {
    #[cfg(debug_assertions)]
    if options.fail_sync && !options.force_staging {
        return Err(std::io::Error::other("injected sync failure"));
    }
    #[cfg(not(debug_assertions))]
    let _ = options;
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .await?
        .sync_all()
        .await
}

fn write_error(error: std::io::Error) -> String {
    AppErrorPayload::disk_write_failed(format!("Could not publish downloaded file: {error}"))
        .command_error()
}

fn publication_error(error: std::io::Error, destination: &Path) -> String {
    if error.kind() == ErrorKind::AlreadyExists {
        AppErrorPayload::final_path_conflict(&destination.to_string_lossy()).command_error()
    } else {
        write_error(error)
    }
}

async fn publish_noreplace(source: &Path, destination: &Path) -> std::io::Result<()> {
    let source = source.to_path_buf();
    let destination = destination.to_path_buf();
    tokio::task::spawn_blocking(move || rename_noreplace(&source, &destination))
        .await
        .map_err(std::io::Error::other)?
}

fn rename_noreplace(source: &Path, destination: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::{
            core::PCWSTR,
            Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH},
        };
        let from: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        // Deliberately omit REPLACE_EXISTING and COPY_ALLOWED: conflicts must
        // fail atomically, and cross-volume copies must use our private staging.
        unsafe {
            MoveFileExW(
                PCWSTR(from.as_ptr()),
                PCWSTR(to.as_ptr()),
                MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|error| std::io::Error::from_raw_os_error(error.code().0 & 0xffff))
    }
    #[cfg(unix)]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let from = CString::new(source.as_os_str().as_bytes())?;
        let to = CString::new(destination.as_os_str().as_bytes())?;
        #[cfg(target_os = "linux")]
        let result = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                from.as_ptr(),
                libc::AT_FDCWD,
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        #[cfg(target_os = "macos")]
        let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            if result == 0 {
                return Ok(());
            }
            let error = std::io::Error::last_os_error();
            if !matches!(
                error.raw_os_error(),
                Some(libc::ENOSYS) | Some(libc::EINVAL) | Some(libc::ENOTSUP)
            ) {
                return Err(error);
            }
        }
        // Older kernels/filesystems can still provide atomic exclusive link
        // creation. Never fall back to a check followed by an overwriting rename.
        std::fs::hard_link(source, destination)?;
        if let Err(error) = std::fs::remove_file(source) {
            tracing::warn!(%error, "published file; temporary hard link cleanup failed");
        }
        Ok(())
    }
}

async fn publish_across_volumes(
    source: &Path,
    destination: &Path,
    mut options: PublishOptions,
) -> Result<(), String> {
    let staging = destination.with_file_name(format!(
        ".{}.{}.staging",
        destination
            .file_name()
            .unwrap_or_default()
            .to_string_lossy(),
        Uuid::new_v4()
    ));
    copy_to_new_file(source, &staging, &options)
        .await
        .map_err(write_error)?;
    before_publish(&mut options);
    if let Err(error) = publish_noreplace(&staging, destination).await {
        let _ = fs::remove_file(&staging).await;
        return Err(publication_error(error, destination));
    }
    if let Err(error) = fs::remove_file(source).await {
        tracing::warn!(%error, "published file; cross-volume source cleanup failed");
    }
    Ok(())
}

async fn copy_to_new_file(
    source: &Path,
    destination: &Path,
    options: &PublishOptions,
) -> std::io::Result<()> {
    let mut input = fs::File::open(source).await?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .await?;
    let result = async {
        let mut buffer = vec![0_u8; 128 * 1024];
        #[cfg(debug_assertions)]
        let mut written = 0;
        loop {
            let read = input.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            #[cfg(debug_assertions)]
            if let Some(limit) = options.fail_copy_after {
                if written + read > limit {
                    output
                        .write_all(&buffer[..limit.saturating_sub(written)])
                        .await?;
                    return Err(std::io::Error::other("injected copy failure"));
                }
            }
            output.write_all(&buffer[..read]).await?;
            #[cfg(debug_assertions)]
            {
                written += read;
            }
        }
        #[cfg(not(debug_assertions))]
        let _ = options;
        #[cfg(debug_assertions)]
        if options.fail_sync {
            return Err(std::io::Error::other("injected staging sync failure"));
        }
        output.sync_all().await
    }
    .await;
    drop(output);
    if result.is_err() {
        let _ = fs::remove_file(destination).await;
    }
    result
}

fn is_cross_device_error(error: &std::io::Error) -> bool {
    error.kind() == ErrorKind::CrossesDevices
}

#[cfg(debug_assertions)]
#[doc(hidden)]
pub mod testing {
    use super::*;
    pub async fn finalize(
        source: &Path,
        destination: &Path,
        force_staging: bool,
        fail_copy_after: Option<usize>,
        fail_sync: bool,
        before: Option<Box<dyn FnOnce() + Send + Sync>>,
    ) -> Result<PathBuf, String> {
        finalize_impl(
            source,
            destination,
            PublishOptions {
                force_staging,
                fail_copy_after,
                fail_sync,
                before_publish: before,
            },
        )
        .await
    }
}

pub(crate) async fn preallocate_temp_file(file: &fs::File, total_size: i64, task_id: &str) {
    if total_size <= 0 {
        return;
    }
    if let Err(error) = file
        .set_len(u64::try_from(total_size).unwrap_or(u64::MAX))
        .await
    {
        tracing::warn!(
            task_id,
            total_size,
            error = %error,
            "failed to preallocate temporary download file"
        );
    }
}

pub(crate) async fn persist_completed_path(
    pool: &SqlitePool,
    task_id: &str,
    completed_path: &Path,
) -> Result<(), String> {
    let file_name = completed_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("download")
        .to_string();
    db::update_task_final_path(pool, task_id, &file_name, &completed_path.to_string_lossy()).await
}

/// ARC-38: removes a directory, treating a missing path as success. Used for
/// staging cleanup on engine completion and startup sweeps — staging holds
/// segment data approaching the final file's size, so leftover directories
/// accumulate tens of GB over a long session. Callers treat failures as
/// best-effort: a completed download must not be reported failed because
/// cleanup hit a locked file (e.g. an antivirus scanner).
pub(crate) async fn remove_dir_all_if_exists(path: &Path) -> Result<(), std::io::Error> {
    if !fs::try_exists(path).await.unwrap_or(false) {
        return Ok(());
    }
    fs::remove_dir_all(path).await
}

#[cfg(test)]
mod tests {
    use super::*;

    // ENG-04: `finalize_download_file` carries ARC-02's atomic publish contract
    // (no-clobber on both the same-volume rename and the cross-volume staging
    // path) and had no direct coverage.

    fn scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vibe-fileops-{label}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[tokio::test]
    async fn finalize_renames_same_volume_and_preserves_bytes() {
        let dir = scratch_dir("rename");
        let temp_path = dir.join("file.bin.vibe-part");
        let final_path = dir.join("file.bin");
        fs::write(&temp_path, b"payload-bytes")
            .await
            .expect("write temp");

        let published = finalize_download_file(&temp_path, &final_path)
            .await
            .expect("publish same volume");

        assert_eq!(published, final_path);
        assert_eq!(
            fs::read(&final_path).await.expect("read published"),
            b"payload-bytes".to_vec()
        );
        assert!(!temp_path.exists(), "temp file should be gone after rename");
        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn finalize_refuses_to_clobber_an_existing_final_file() {
        let dir = scratch_dir("noclobber");
        let temp_path = dir.join("file.bin.vibe-part");
        let final_path = dir.join("file.bin");
        fs::write(&temp_path, b"new").await.expect("write temp");
        fs::write(&final_path, b"existing")
            .await
            .expect("write existing");

        let error = finalize_download_file(&temp_path, &final_path)
            .await
            .expect_err("publish must not clobber");
        assert!(
            error.contains("final_path_conflict"),
            "expected a structured conflict, got: {error}"
        );
        // The file that was already there must survive untouched.
        assert_eq!(
            fs::read(&final_path).await.expect("read existing"),
            b"existing".to_vec()
        );
        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn staging_copy_uses_create_new_and_preserves_existing_file() {
        let dir = scratch_dir("staging-noclobber");
        let source = dir.join("source.bin");
        let destination = dir.join("destination.staging");
        fs::write(&source, b"new").await.expect("write source");
        fs::write(&destination, b"existing")
            .await
            .expect("write destination");

        let error = copy_to_new_file(&source, &destination, &PublishOptions::default())
            .await
            .expect_err("staging copy must refuse an existing path");
        assert_eq!(error.kind(), ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read(&destination).await.expect("read destination"),
            b"existing"
        );
        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn remove_dir_all_if_exists_treats_missing_path_as_success() {
        let root = scratch_dir("cleanup");
        let dir = root.join("nested").join("staging");
        remove_dir_all_if_exists(&dir)
            .await
            .expect("missing path is success");

        fs::create_dir_all(&dir).await.expect("create staging");
        fs::write(dir.join("seg-1.bin"), b"x")
            .await
            .expect("write segment");
        remove_dir_all_if_exists(&dir)
            .await
            .expect("remove staging");
        assert!(!dir.exists(), "staging should be gone");
        let _ = fs::remove_dir_all(root).await;
    }
}
