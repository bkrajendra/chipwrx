//! `NFR-D3`: "version and git SHA surfaced in About and in the diagnostics bundle." The
//! diagnostics bundle side is `core::toolchain::diagnostics`; this is the About-screen side.

use serde::Serialize;
use tauri::AppHandle;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub git_sha: String,
}

#[tauri::command]
pub fn app_info_get(app: AppHandle) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        git_sha: env!("VIBE_GIT_SHA").to_string(),
    }
}
