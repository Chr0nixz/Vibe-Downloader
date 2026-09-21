//! ARC-49: exercise the shared finalizer with an independent competing writer.
#![cfg(debug_assertions)]

use std::{io::Write, process::Command};
use tauri_app_lib::download::publication_testing::finalize;

mod common;

#[test]
fn external_writer() {
    let Some(path) = std::env::var_os("VIBE_TEST_FINAL_DESTINATION") else {
        return;
    };
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(b"external bytes").unwrap();
    file.sync_all().unwrap();
}

#[tokio::test]
async fn competing_process_never_loses_its_file_on_either_publication_path() {
    for staged in [false, true] {
        let paths = common::TestPaths::new("publication-race");
        let source = paths.temp.parent().unwrap().join("download.part");
        let dest = paths.temp.parent().unwrap().join("final.bin");
        std::fs::write(&source, b"download bytes").unwrap();
        let target = dest.clone();
        let hook = Box::new(move || {
            assert!(Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "external_writer", "--nocapture"])
                .env("VIBE_TEST_FINAL_DESTINATION", target)
                .status()
                .unwrap()
                .success());
        });
        let error = finalize(&source, &dest, staged, None, false, Some(hook))
            .await
            .unwrap_err();
        assert!(error.contains("final_path_conflict"), "{error}");
        assert_eq!(std::fs::read(&source).unwrap(), b"download bytes");
        assert_eq!(std::fs::read(&dest).unwrap(), b"external bytes");
        assert_eq!(
            std::fs::read_dir(paths.temp.parent().unwrap())
                .unwrap()
                .count(),
            2
        );
    }
}

#[tokio::test]
async fn partial_copy_and_sync_failures_preserve_source_and_remove_staging() {
    for (staged, copy_failure, sync_failure) in [
        (true, Some(17), false),
        (true, None, true),
        (false, None, true),
    ] {
        let paths = common::TestPaths::new("publication-failure");
        let source = paths.temp.parent().unwrap().join("download.part");
        let dest = paths.temp.parent().unwrap().join("final.bin");
        let bytes = vec![42_u8; 200_000];
        std::fs::write(&source, &bytes).unwrap();
        assert!(
            finalize(&source, &dest, staged, copy_failure, sync_failure, None)
                .await
                .unwrap_err()
                .contains("disk_write_failed")
        );
        assert_eq!(std::fs::read(&source).unwrap(), bytes);
        assert!(!dest.exists());
        assert_eq!(
            std::fs::read_dir(paths.temp.parent().unwrap())
                .unwrap()
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn successful_native_and_staged_publication_move_all_bytes() {
    for staged in [false, true] {
        let paths = common::TestPaths::new("publication-success");
        let source = paths.temp.parent().unwrap().join("download.part");
        let dest = paths.temp.parent().unwrap().join("final.bin");
        let bytes = vec![91_u8; 200_000];
        std::fs::write(&source, &bytes).unwrap();
        finalize(&source, &dest, staged, None, false, None)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), bytes);
        assert!(!source.exists());
        assert_eq!(
            std::fs::read_dir(paths.temp.parent().unwrap())
                .unwrap()
                .count(),
            1
        );
    }
}
