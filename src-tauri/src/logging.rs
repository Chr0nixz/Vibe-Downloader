use std::fmt::{self, Write as FmtWrite};
use std::sync::Once;

use crate::platform;
use tauri::{AppHandle, Manager, Runtime};
use tracing::Level;
use tracing_subscriber::{
    filter::EnvFilter,
    layer::SubscriberExt,
    util::SubscriberInitExt,
    Layer,
};

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

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_filter));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(LogBridgeLayer)
        .init();

    let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|e| format!("Failed to resolve app log directory: {e}"))?;
    tracing::info!(log_dir = %log_dir.display(), "logging initialized");
    Ok(())
}

pub fn init_standalone_logging() -> Result<(), String> {
    let mut init_error: Option<String> = None;
    INIT.call_once(|| {
        if let Err(error) = init_standalone_logging_inner() {
            init_error = Some(error);
        }
    });
    init_error.map_or(Ok(()), Err)
}

fn init_standalone_logging_inner() -> Result<(), String> {
    let log_dir = platform::app_log_dir()?;
    std::fs::create_dir_all(&log_dir)
        .map_err(|e| format!("Failed to create log directory: {e}"))?;

    let default_filter = if cfg!(debug_assertions) {
        "vibe_downloader=debug"
    } else {
        "vibe_downloader=info"
    };

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_filter));

    let file_appender = tracing_appender::rolling::RollingFileAppender::new(
        tracing_appender::rolling::Rotation::DAILY,
        &log_dir,
        "native-host",
    );
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    std::mem::forget(guard);

    tracing_subscriber::registry()
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
        )
        .init();

    tracing::info!(log_dir = %log_dir.display(), "native host logging initialized");
    Ok(())
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
