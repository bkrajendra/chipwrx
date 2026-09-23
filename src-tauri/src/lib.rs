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
    // `M9`/`NFR-R2`: "every spawned child is registered in a process table and killed on
    // app exit and on panic." `RunEvent::Exit` (below) covers a normal exit; a panic
    // anywhere in the app needs its own hook, since nothing else runs after one unwinds off
    // the last frame (or, with this crate's release-profile `panic = "abort"`, before the
    // abort — panic hooks still run either way). Chains the previous hook (rather than
    // replacing it) so `tauri_plugin_log`'s own panic logging, if any is installed later,
    // still happens.
    let supervisor = ProcessSupervisor::new();
    let supervisor_for_panic = supervisor.clone();
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        supervisor_for_panic.kill_all();
        previous_hook(info);
    }));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                // `NFR-R1`/`M9`: "the redaction filter runs before anything hits disk or
                // the UI" (`core::redact`'s own doc comment already promised this for M9).
                // Mirrors `tauri_plugin_log`'s own default format (`Builder::default`) —
                // this only exists to redact `message` first.
                .format(|out, message, record| {
                    let redacted = crate::core::redact::redact(&message.to_string(), false);
                    out.finish(format_args!(
                        "{}[{}][{}] {}",
                        chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                        record.target(),
                        record.level(),
                        redacted
                    ))
                })
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(supervisor)
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

            // `M9`/`NFR-R2`: reap whatever the *previous* run's process registry still
            // lists (non-empty only if that run was killed before it reached
            // `RunEvent::Exit`'s `kill_all()`) before this session starts tracking its own
            // processes in the same file.
            match commands::util::cache_dir(app.handle()) {
                Ok(cache_dir) => {
                    let registry_path = cache_dir.join("running-procs.json");
                    for label in core::proc::reap_orphans_from_previous_run(&registry_path) {
                        tracing::warn!("reaped an orphaned process left by a previous run: {label}");
                    }
                    app.state::<ProcessSupervisor>().set_registry_path(registry_path);
                }
                Err(e) => tracing::warn!("failed to resolve cache dir; orphan reaping disabled this session: {e}"),
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info::app_info_get,
            commands::doctor::doctor_run,
            commands::doctor::doctor_get_cached,
            commands::doctor::toolchain_install,
            commands::doctor::toolchain_set_path,
            commands::doctor::toolchain_open_auth_terminal,
            commands::doctor::diagnostics_export,
            commands::settings::settings_get_global,
            commands::settings::settings_set_global,
            commands::secrets::secrets_has_api_key,
            commands::secrets::secrets_set_api_key,
            commands::secrets::secrets_clear_api_key,
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
            commands::pipeline::pipeline_check,
            commands::pipeline::pipeline_test,
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
