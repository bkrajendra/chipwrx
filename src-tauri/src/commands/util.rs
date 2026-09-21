//! Small helpers shared across command modules.

use crate::error::AppError;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// Snapshot/diff/revert operations (`core::snapshot::engine`) do synchronous filesystem +
/// libgit2 I/O — potentially more than the ~50ms `IPC-CONTRACT.md` §10 rule 3 budgets for
/// a command that doesn't stream, especially on a large workspace. Runs `f` on a blocking
/// thread instead of the async runtime's worker.
pub async fn run_blocking<F, T>(f: F) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| AppError::SnapshotFailed {
            message: format!("background task panicked: {e}"),
        })?
}

pub fn cache_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    app.path().app_cache_dir().map_err(|e| AppError::Io {
        message: format!("resolving app cache directory: {e}"),
    })
}

/// An app-owned scratch directory — never the user's workspace. Used as `cwd` for probes,
/// board lookups, and other spawns that aren't tied to a specific project.
pub fn scratch_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    let dir = cache_dir(app)?.join("scratch");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
