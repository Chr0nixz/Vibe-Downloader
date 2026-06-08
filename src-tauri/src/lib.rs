pub mod commands;
pub mod db;
pub mod download;
pub mod events;
pub mod logging;
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
    pub source_host: String,
    pub connection_slots: usize,
}

pub struct AppState {
    pub pool: SqlitePool,
    pub downloads: Arc<Mutex<HashMap<String, DownloadControl>>>,
    pub scheduler: Arc<Mutex<()>>,
    pub speed_limiter: Arc<download::GlobalSpeedLimiter>,
}

fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    use tauri_specta::{collect_commands, Builder};

    #[cfg(debug_assertions)]
    let builder = Builder::<tauri::Wry>::new()
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
        ]);

    #[cfg(not(debug_assertions))]
    let builder = Builder::<tauri::Wry>::new()
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
        ]);

    builder
        .typ::<models::AppErrorPayload>()
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

    let builder = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets({
                    let mut targets = vec![
                        tauri_plugin_log::Target::new(
                            tauri_plugin_log::TargetKind::LogDir {
                                file_name: Some("vibe".to_string()),
                            },
                        ),
                        tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Webview),
                    ];
                    if cfg!(debug_assertions) {
                        targets.push(tauri_plugin_log::Target::new(
                            tauri_plugin_log::TargetKind::Stdout,
                        ));
                    }
                    targets
                })
                .level(log::LevelFilter::Trace)
                .build(),
        )
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            tracing::info!(args = ?args, "single-instance launch received");
            process_browser_handoff_files_from_args(app, args, "single-instance");
            focus_main_window(app);
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_os::init());

    #[cfg(debug_assertions)]
    let builder = builder.invoke_handler(tauri::generate_handler![
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
    ]);

    #[cfg(not(debug_assertions))]
    let builder = builder.invoke_handler(tauri::generate_handler![
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
    ]);

    builder
        .setup(|app| {
            logging::init_logging(app.handle())?;

            let handle = app.handle().clone();

            let db_path = platform::db_path(&handle)?;
            let pool = tauri::async_runtime::block_on(async { db::connect(&db_path).await })?;
            tauri::async_runtime::block_on(async { db::reset_interrupted_tasks(&pool).await })?;
            let default_dir = commands::settings::default_download_dir(&handle)?;
            let settings =
                tauri::async_runtime::block_on(async { db::get_settings(&pool, default_dir).await })?;
            let speed_limiter = Arc::new(download::GlobalSpeedLimiter::new(
                db::parse_speed_limit_bps(settings.global_speed_limit_bps.as_deref()),
            ));

            app.manage(AppState {
                pool: pool.clone(),
                downloads: Arc::new(Mutex::new(HashMap::new())),
                scheduler: Arc::new(Mutex::new(())),
                speed_limiter,
            });
            process_browser_handoff_files_from_args(
                &handle,
                std::env::args().collect(),
                "initial-launch",
            );

            if let Some(window) = app.get_webview_window("main") {
                platform::configure_main_window(&window)?;
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn process_browser_handoff_files_from_args(
    app: &tauri::AppHandle,
    args: Vec<String>,
    source: &'static str,
) {
    let files = browser_handoff_files_from_args(args);
    if files.is_empty() {
        return;
    }
    tracing::info!(count = files.len(), source, "browser handoff files received");

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();
        for path in files {
            match commands::browser::read_handoff_file(&path) {
                Ok(input) => {
                    let request_id = input.request_id.clone();
                    tracing::info!(
                        request_id = %request_id,
                        path = %path.display(),
                        source,
                        "processing browser handoff file"
                    );
                    if let Err(error) = commands::browser::create_browser_handoff_task_with_state(
                        handle.clone(),
                        state.inner(),
                        input,
                    )
                    .await
                    {
                        tracing::error!(
                            request_id = %request_id,
                            path = %path.display(),
                            source,
                            error = %error,
                            "browser handoff task creation failed"
                        );
                    } else if let Err(error) = std::fs::remove_file(&path) {
                        tracing::warn!(
                            request_id = %request_id,
                            path = %path.display(),
                            source,
                            error = %error,
                            "browser handoff file cleanup failed"
                        );
                    }
                }
                Err(error) => {
                    tracing::error!(
                        path = %path.display(),
                        source,
                        error = %error,
                        "browser handoff file read failed"
                    );
                }
            }
        }
    });
}

fn focus_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
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
