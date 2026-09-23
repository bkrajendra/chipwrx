//! Thin Tauri command handlers for `GlobalSettings`. See `IPC-CONTRACT.md` §9. Logic
//! (atomic read/write, corruption recovery) lives in `core::settings`.

use crate::core::settings::{self, GlobalSettings};
use crate::error::AppError;
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex;

/// Loaded once at startup from disk (`core/settings`) and kept in memory; every write goes
/// back through `core::settings::save` before this is updated, so a crash mid-write never
/// leaves the in-memory copy ahead of what's on disk.
pub struct SettingsState(pub Mutex<GlobalSettings>);

pub fn config_dir(app: &AppHandle) -> Result<std::path::PathBuf, AppError> {
    app.path().app_config_dir().map_err(|e| AppError::Io {
        message: format!("resolving app config directory: {e}"),
    })
}

pub fn load_at_startup(app: &AppHandle) -> Result<GlobalSettings, AppError> {
    let dir = config_dir(app)?;
    let (mut loaded, _outcome) = settings::load(&dir)?;

    let current_version = app.package_info().version.to_string();
    if settings::apply_version_migration(&mut loaded, &current_version) {
        settings::save(&dir, &loaded)?;
    }

    Ok(loaded)
}

#[tauri::command]
pub async fn settings_get_global(state: State<'_, SettingsState>) -> Result<GlobalSettings, AppError> {
    Ok(state.0.lock().await.clone())
}

/// A patch is any subset of `GlobalSettings`' top-level sections; unlike the full struct,
/// every field is optional so the frontend only sends what changed.
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSettingsPatch {
    pub toolchain: Option<crate::core::settings::ToolchainSettings>,
    pub claude: Option<crate::core::settings::ClaudeSettings>,
    pub pipeline: Option<crate::core::settings::PipelineSettings>,
    pub monitor: Option<crate::core::settings::MonitorSettings>,
    pub logs: Option<crate::core::settings::LogSettings>,
    pub editor: Option<crate::core::settings::EditorSettings>,
    pub appearance: Option<crate::core::settings::AppearanceSettings>,
    pub network: Option<crate::core::settings::NetworkSettings>,
    pub advanced: Option<crate::core::settings::AdvancedSettings>,
    pub onboarding_completed: Option<bool>,
}

#[tauri::command]
pub async fn settings_set_global(
    app: AppHandle,
    state: State<'_, SettingsState>,
    patch: GlobalSettingsPatch,
) -> Result<GlobalSettings, AppError> {
    let mut guard = state.0.lock().await;
    if let Some(v) = patch.toolchain {
        guard.toolchain = v;
    }
    if let Some(v) = patch.claude {
        guard.claude = v;
    }
    if let Some(v) = patch.pipeline {
        guard.pipeline = v;
    }
    if let Some(v) = patch.monitor {
        guard.monitor = v;
    }
    if let Some(v) = patch.logs {
        guard.logs = v;
    }
    if let Some(v) = patch.editor {
        guard.editor = v;
    }
    if let Some(v) = patch.appearance {
        guard.appearance = v;
    }
    if let Some(v) = patch.network {
        guard.network = v;
    }
    if let Some(v) = patch.advanced {
        guard.advanced = v;
    }
    if let Some(v) = patch.onboarding_completed {
        guard.onboarding_completed = v;
    }

    let dir = config_dir(&app)?;
    settings::save(&dir, &guard)?;
    Ok(guard.clone())
}
