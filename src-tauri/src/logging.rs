//! Structured logging.
//!
//! Human-readable logs go to stdout; JSON logs go to a daily-rotated file in
//! the app log directory. Log lines use an `event` field with stable
//! UPPER_SNAKE_CASE names (e.g. `APP_STARTED`, `SETTINGS_UPDATED`) so they
//! can be filtered and audited. Never log secrets or message contents.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialise logging. The returned guard must be kept alive for the
/// lifetime of the app so buffered file logs are flushed.
pub fn init(log_dir: Option<&Path>, filter: Option<&str>) -> Option<WorkerGuard> {
    let filter = EnvFilter::try_new(filter.unwrap_or("info")).unwrap_or_else(|_| EnvFilter::new("info"));
    let stdout_layer = fmt::layer().with_target(false).compact();

    let (file_layer, guard) = match log_dir {
        Some(dir) if std::fs::create_dir_all(dir).is_ok() => {
            let appender = tracing_appender::rolling::daily(dir, "igris.log");
            let (writer, guard) = tracing_appender::non_blocking(appender);
            (Some(fmt::layer().json().with_writer(writer).with_current_span(false)), Some(guard))
        }
        _ => (None, None),
    };

    // `try_init` so tests or double-initialisation don't panic.
    let _ = tracing_subscriber::registry().with(filter).with(stdout_layer).with(file_layer).try_init();
    guard
}
