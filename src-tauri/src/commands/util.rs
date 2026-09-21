//! Small helpers shared across command modules.

use crate::error::AppError;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

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
