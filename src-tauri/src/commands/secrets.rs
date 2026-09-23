//! Thin Tauri command handlers over `core::secrets` (`NFR-S1`). Never returns the key
//! value itself to the frontend — only whether one is set.

use crate::commands::util::run_blocking;
use crate::core::secrets;
use crate::error::AppError;

#[tauri::command]
pub async fn secrets_has_api_key() -> Result<bool, AppError> {
    run_blocking(secrets::has_anthropic_api_key).await
}

#[tauri::command]
pub async fn secrets_set_api_key(key: String) -> Result<(), AppError> {
    run_blocking(move || secrets::set_anthropic_api_key(&key)).await
}

#[tauri::command]
pub async fn secrets_clear_api_key() -> Result<(), AppError> {
    run_blocking(secrets::clear_anthropic_api_key).await
}
