pub mod commands;
pub mod db;
pub mod download;
pub mod events;
pub mod models;
pub mod platform;

use std::{
    collections::HashMap,
    sync::{atomic::AtomicBool, Arc},
};

use sqlx::SqlitePool;
use tauri::Manager;
use tokio::{sync::Mutex, task::JoinHandle};

pub struct DownloadControl {
    pub cancel: Arc<AtomicBool>,
    pub handle: JoinHandle<()>,
}

pub struct AppState {
    pub pool: SqlitePool,
    pub downloads: Arc<Mutex<HashMap<String, DownloadControl>>>,
    pub scheduler: Arc<Mutex<()>>,
}

fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    use tauri_specta::{collect_commands, Builder};

    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::tasks::list_tasks,
            commands::tasks::get_task,
            commands::tasks::list_task_segments,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::browser::get_browser_integration_status,
            commands::browser::install_browser_integration,
            commands::browser::uninstall_browser_integration,
            commands::browser::create_browser_handoff_task,
            commands::tasks::probe_task,
            commands::tasks::create_task,
            commands::tasks::pause_task,
            commands::tasks::resume_task,
            commands::tasks::retry_task,
            commands::tasks::cancel_task,
            commands::tasks::delete_task,
            commands::tasks::open_task_file,
            commands::tasks::open_task_folder,
            commands::tasks::seed_mock_tasks,
        ])
        .typ::<models::AppSettings>()
        .typ::<models::TaskProgressPayload>()
        .typ::<models::BrowserIntegrationStatus>()
        .typ::<models::BrowserHandoffResult>()
}

pub fn export_typescript_bindings() -> Result<(), Box<dyn std::error::Error>> {
    use specta_typescript::Typescript;

    let bindings_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("src")
        .join("generated")
        .join("bindings.ts");

    specta_builder().export(Typescript::default(), bindings_path)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(debug_assertions)]
    {
        export_typescript_bindings().expect("Failed to export TypeScript bindings");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_os::init())
        .invoke_handler(tauri::generate_handler![
            commands::tasks::list_tasks,
            commands::tasks::get_task,
            commands::tasks::list_task_segments,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::browser::get_browser_integration_status,
            commands::browser::install_browser_integration,
            commands::browser::uninstall_browser_integration,
            commands::browser::create_browser_handoff_task,
            commands::tasks::probe_task,
            commands::tasks::create_task,
            commands::tasks::pause_task,
            commands::tasks::resume_task,
            commands::tasks::retry_task,
            commands::tasks::cancel_task,
            commands::tasks::delete_task,
            commands::tasks::open_task_file,
            commands::tasks::open_task_folder,
            commands::tasks::seed_mock_tasks,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            let db_path = platform::db_path(&handle)?;
            let pool = tauri::async_runtime::block_on(async { db::connect(&db_path).await })?;
            tauri::async_runtime::block_on(async { db::reset_interrupted_tasks(&pool).await })?;

            app.manage(AppState {
                pool: pool.clone(),
                downloads: Arc::new(Mutex::new(HashMap::new())),
                scheduler: Arc::new(Mutex::new(())),
            });
            process_initial_browser_handoff_files(&handle);

            if let Some(window) = app.get_webview_window("main") {
                platform::configure_main_window(&window)?;
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn process_initial_browser_handoff_files(app: &tauri::AppHandle) {
    let files = browser_handoff_files_from_args(std::env::args().collect());
    if files.is_empty() {
        return;
    }

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();
        for path in files {
            match commands::browser::read_handoff_file(&path) {
                Ok(input) => {
                    let _ = commands::browser::create_browser_handoff_task_with_state(
                        handle.clone(),
                        state.inner(),
                        input,
                    )
                    .await;
                }
                Err(error) => {
                    eprintln!("{error}");
                }
            }
        }
    });
}

fn browser_handoff_files_from_args(args: Vec<String>) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        if arg == "--browser-handoff-file" {
            if let Some(path) = iter.next() {
                files.push(std::path::PathBuf::from(path));
            }
        }
    }
    files
}
