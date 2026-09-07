use std::fmt::{self, Write as FmtWrite};
use std::sync::Once;

use crate::platform;
use tauri::{AppHandle, Manager, Runtime};
use tracing::Level;
use tracing_subscriber::{filter::EnvFilter, layer::SubscriberExt, Layer};

static INIT: Once = Once::new();

pub fn init_logging<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let mut init_error: Option<String> = None;
    INIT.call_once(|| {
        if let Err(error) = init_logging_inner(app) {
            init_error = Some(error);
        }
    });
    init_error.map_or(Ok(()), Err)
}

fn init_logging_inner<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let default_filter = if cfg!(debug_assertions) {
        "vibe_downloader=debug,tauri=warn,sqlx=warn"
    } else {
        "vibe_downloader=info,tauri=warn,sqlx=warn"
    };

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));

    let subscriber = tracing_subscriber::registry()
        .with(env_filter)
        .with(LogBridgeLayer);

    if let Err(error) = tracing::subscriber::set_global_default(subscriber) {
        eprintln!("tracing subscriber already initialized: {error}");
    }

    let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|e| format!("Failed to resolve app log directory: {e}"))?;
    tracing::info!(log_dir = %log_dir.display(), "logging initialized");
    Ok(())
}

/// PERF-12: the caller must keep this alive for the whole process. Dropping it
/// flushes the non-blocking writer; leaking it (the previous `mem::forget`)
/// meant a short-lived native host could exit with its last lines - usually the
/// error that explains the failure - still buffered and never written.
///
/// `None` when logging was already initialized by an earlier call.
pub type StandaloneLogGuard = Option<tracing_appender::non_blocking::WorkerGuard>;

#[must_use = "dropping the guard immediately would stop the log writer"]
pub fn init_standalone_logging() -> Result<StandaloneLogGuard, String> {
    let mut init_error: Option<String> = None;
    let mut guard: StandaloneLogGuard = None;
    INIT.call_once(|| match init_standalone_logging_inner() {
        Ok(worker_guard) => guard = Some(worker_guard),
        Err(error) => init_error = Some(error),
    });
    match init_error {
        Some(error) => Err(error),
        None => Ok(guard),
    }
}

fn init_standalone_logging_inner() -> Result<tracing_appender::non_blocking::WorkerGuard, String> {
    let log_dir = platform::app_log_dir()?;
    std::fs::create_dir_all(&log_dir)
        .map_err(|e| format!("Failed to create log directory: {e}"))?;

    let default_filter = if cfg!(debug_assertions) {
        "vibe_downloader=debug"
    } else {
        "vibe_downloader=info"
    };

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));

    // PERF-12: daily rotation without `max_log_files` never deletes anything.
    // The native host is spawned by the browser on every handoff, so the log
    // directory would grow without bound.
    let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("native-host")
        .max_log_files(7)
        .build(&log_dir)
        .map_err(|e| format!("Failed to create the log file appender: {e}"))?;
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let subscriber = tracing_subscriber::registry()
        .with(env_filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_target(true),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(false)
                .with_target(true),
        );

    if let Err(error) = tracing::subscriber::set_global_default(subscriber) {
        eprintln!("tracing subscriber already initialized: {error}");
    }

    tracing::info!(log_dir = %log_dir.display(), "native host logging initialized");
    Ok(guard)
}

pub fn sanitize_url(url: &str) -> String {
    let trimmed = url.trim();
    if let Ok(parsed) = reqwest::Url::parse(trimmed) {
        let mut sanitized = format!(
            "{}://{}{}",
            parsed.scheme(),
            parsed.host_str().unwrap_or(""),
            parsed.path()
        );
        if let Some(port) = parsed.port() {
            sanitized = format!(
                "{}://{}:{}{}",
                parsed.scheme(),
                parsed.host_str().unwrap_or(""),
                port,
                parsed.path()
            );
        }
        sanitized
    } else if let Some((base, _)) = trimmed.split_once('?') {
        base.to_string()
    } else {
        trimmed.to_string()
    }
}

struct LogBridgeLayer;

impl<S> Layer<S> for LogBridgeLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let level = match *event.metadata().level() {
            Level::ERROR => log::Level::Error,
            Level::WARN => log::Level::Warn,
            Level::INFO => log::Level::Info,
            Level::DEBUG => log::Level::Debug,
            Level::TRACE => log::Level::Trace,
        };

        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);
        let target = event.metadata().target();
        if visitor.fields.is_empty() {
            log::log!(target: target, level, "{}", visitor.message);
        } else {
            log::log!(target: target, level, "{} {}", visitor.message, visitor.fields);
        }
    }
}

#[derive(Default)]
struct EventVisitor {
    message: String,
    fields: String,
}

impl tracing::field::Visit for EventVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}").trim_matches('"').to_string();
            return;
        }
        if !self.fields.is_empty() {
            let _ = write!(self.fields, " ");
        }
        let _ = write!(self.fields, "{}={value:?}", field.name());
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
            return;
        }
        if !self.fields.is_empty() {
            let _ = write!(self.fields, " ");
        }
        let _ = write!(self.fields, "{}={value:?}", field.name());
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        if !self.fields.is_empty() {
            let _ = write!(self.fields, " ");
        }
        let _ = write!(self.fields, "{}={value}", field.name());
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        if !self.fields.is_empty() {
            let _ = write!(self.fields, " ");
        }
        let _ = write!(self.fields, "{}={value}", field.name());
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        if !self.fields.is_empty() {
            let _ = write!(self.fields, " ");
        }
        let _ = write!(self.fields, "{}={value}", field.name());
    }
}
