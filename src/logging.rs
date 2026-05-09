//! File-based tracing setup. Stdout is owned by the TUI, so we log to a file
//! under the XDG state dir.

use std::path::PathBuf;

use directories::ProjectDirs;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt};

/// Initialize a non-blocking file appender. Returns the WorkerGuard, which must
/// be kept alive for the lifetime of the program (drop = flush).
pub fn init() -> std::io::Result<WorkerGuard> {
    let log_dir = log_dir();
    std::fs::create_dir_all(&log_dir)?;

    let appender = tracing_appender::rolling::never(&log_dir, "postui.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(appender);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .init();

    tracing::info!(dir = %log_dir.display(), "tracing initialized");
    Ok(guard)
}

fn log_dir() -> PathBuf {
    if let Some(dirs) = ProjectDirs::from("", "", "postui") {
        // ProjectDirs gives data_local_dir; we want state_dir on Linux.
        // Fall back to data_local_dir if state_dir isn't available.
        if let Some(state) = dirs.state_dir() {
            return state.to_path_buf();
        }
        return dirs.data_local_dir().to_path_buf();
    }
    PathBuf::from(".")
}
