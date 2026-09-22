//! Thin Tauri command handlers for serial device listing. See `IPC-CONTRACT.md` §6 —
//! `device_list` matches that signature exactly (`() -> Vec<SerialDevice>`) so M6's
//! `PortBroker` can add leasing on top without renaming it. `device_set_preferred_port`
//! isn't in `IPC-CONTRACT.md` (`SPEC.md` §8 open question 21) — `pipeline_upload` needs
//! *some* way to read a chosen port back out, and `ProjectSettings.device.preferredPort`
//! already exists in the data model (M2) for exactly this.

use crate::commands::project::{entry_path, ProjectRegistryState};
use crate::commands::settings::SettingsState;
use crate::commands::util::scratch_dir;
use crate::core::device::list::{self, SerialDevice};
use crate::core::project::workspace;
use crate::core::proc::ProcessSupervisor;
use crate::core::toolchain::probes;
use crate::core::toolchain::resolve::Tool;
use crate::error::AppError;
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn device_list(
    app: AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
) -> Result<Vec<SerialDevice>, AppError> {
    let pio_path = settings_state.0.lock().await.toolchain.pio_path.clone();
    let Some(pio) = probes::resolve_tool(Tool::Pio, pio_path.as_deref(), &supervisor).await else {
        return Err(AppError::ToolMissing {
            tool: "pio".into(),
            install_action: true,
        });
    };
    let cwd = scratch_dir(&app)?;
    list::list_serial_devices(&supervisor, &pio, &cwd).await
}

#[tauri::command]
pub async fn device_set_preferred_port(
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
    port: Option<String>,
) -> Result<(), AppError> {
    let dir = {
        let reg = registry_state.0.lock().await;
        entry_path(&reg, &workspace)?
    };
    let mut settings = crate::core::project::workspace::load(&dir)?.ok_or_else(|| AppError::Io {
        message: format!("{} has no .vibe/project.json", dir.display()),
    })?;
    settings.device.preferred_port = port;
    workspace::save(&dir, &settings)?;
    Ok(())
}
