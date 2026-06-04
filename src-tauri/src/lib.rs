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
}

fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    use tauri_specta::{collect_commands, Builder};

    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::tasks::list_tasks,
            commands::tasks::get_task,
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
        .typ::<models::TaskProgressPayload>()
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
        .plugin(tauri_plugin_os::init())
        .invoke_handler(tauri::generate_handler![
            commands::tasks::list_tasks,
            commands::tasks::get_task,
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
            });

            if let Some(window) = app.get_webview_window("main") {
                platform::configure_main_window(&window)?;
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

