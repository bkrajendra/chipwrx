pub mod commands;
pub mod core;
pub mod error;

use crate::commands::claude::ActiveTurnsState;
use crate::commands::device::{DeviceFocusState, DeviceListState};
use crate::commands::doctor::{DoctorState, HttpClientState};
use crate::commands::monitor::MonitorState;
use crate::commands::pipeline::{ActivePipelineProcs, PipelineRegistryState};
use crate::commands::project::ProjectRegistryState;
use crate::commands::settings::SettingsState;
use crate::core::device::broker::PortBroker;
use crate::core::device::telemetry::EsptoolEntrypointCache;
use crate::core::pio::pipeline::PipelineRegistry;
use crate::core::proc::ProcessSupervisor;
use crate::core::project::types::ProjectRegistry;
use crate::core::settings::GlobalSettings;
use crate::core::toolchain::doctor::DoctorCache;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use tauri::{Manager, RunEvent, WindowEvent};
use tokio::sync::Mutex;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .manage(ProcessSupervisor::new())
        .manage(DoctorState(Mutex::new(DoctorCache::new())))
        .manage(HttpClientState(reqwest::Client::new()))
        .manage(ActiveTurnsState(Mutex::new(HashMap::new())))
        .manage(PipelineRegistryState(Mutex::new(PipelineRegistry::new())))
        .manage(ActivePipelineProcs(Mutex::new(HashMap::new())))
        .manage(PortBroker::new())
        .manage(EsptoolEntrypointCache::new())
        .manage(MonitorState::new())
        .manage(DeviceListState(Mutex::new(Vec::new())))
        .manage(DeviceFocusState::default())
        .on_window_event(|window, event| {
            // Drives `FR-DEV-1`'s 2s-focused / 10s-background poll cadence.
            if let WindowEvent::Focused(focused) = event {
                window.state::<DeviceFocusState>().0.store(*focused, Ordering::Relaxed);
            }
        })
        .setup(|app| {
            tauri::async_runtime::spawn(commands::device::run_hotplug_poll_loop(app.handle().clone()));
            let settings = commands::settings::load_at_startup(app.handle()).unwrap_or_else(|e| {
                tracing::warn!("failed to load settings.json, using defaults: {e}");
                GlobalSettings::default()
            });
            app.manage(SettingsState(Mutex::new(settings)));

            let registry = commands::project::load_registry_at_startup(app.handle()).unwrap_or_else(|e| {
                tracing::warn!("failed to load projects.json, using an empty registry: {e}");
                ProjectRegistry::default()
            });
            app.manage(ProjectRegistryState(Mutex::new(registry)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::doctor::doctor_run,
            commands::doctor::doctor_get_cached,
            commands::doctor::toolchain_install,
            commands::doctor::toolchain_set_path,
            commands::doctor::toolchain_open_auth_terminal,
            commands::doctor::diagnostics_export,
            commands::settings::settings_get_global,
            commands::settings::settings_set_global,
            commands::project::boards_list,
            commands::project::project_create,
            commands::project::project_cancel_create,
            commands::project::project_open,
            commands::project::project_list,
            commands::project::project_forget,
            commands::project::project_trust,
            commands::project::project_scan_trust,
            commands::project::project_list_envs,
            commands::project::project_set_env,
            commands::project::project_open_in_editor,
            commands::project::project_reveal,
            commands::project::project_regenerate_claude_md,
            commands::claude::claude_send_turn,
            commands::claude::claude_stop_turn,
            commands::claude::claude_new_session,
            commands::claude::claude_history,
            commands::claude::attachment_add,
            commands::changes::changes_for_turn,
            commands::changes::changes_diff,
            commands::changes::changes_revert_file,
            commands::changes::changes_revert_turn,
            commands::changes::changes_reset_to_last_good_build,
            commands::changes::file_read,
            commands::pipeline::pipeline_build,
            commands::pipeline::pipeline_upload,
            commands::pipeline::pipeline_run_target,
            commands::pipeline::pipeline_stop,
            commands::pipeline::pipeline_targets,
            commands::pipeline::pipeline_state,
            commands::device::device_list,
            commands::device::device_set_preferred_port,
            commands::device::device_select,
            commands::device::device_telemetry,
            commands::monitor::monitor_start,
            commands::monitor::monitor_stop,
            commands::monitor::monitor_send,
            commands::monitor::monitor_save_log,
            commands::monitor::monitor_open_external,
            commands::ini::ini_schema,
            commands::ini::ini_read,
            commands::ini::ini_apply,
            commands::ini::ini_write_raw,
            commands::ini::ini_lint,
            commands::ini::ini_template_save,
            commands::ini::ini_template_list,
            commands::ini::ini_template_apply,
            commands::packages::pkg_search,
            commands::packages::pkg_install,
            commands::packages::pkg_uninstall,
            commands::packages::pkg_installed,
            commands::packages::pkg_outdated,
            commands::packages::pio_settings_get,
            commands::packages::pio_settings_set,
            commands::packages::pio_system_prune,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let RunEvent::Exit = event {
                app_handle.state::<ProcessSupervisor>().kill_all();
            }
        });
}
