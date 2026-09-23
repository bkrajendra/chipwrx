//! Thin Tauri command handlers for serial device listing, selection, and telemetry. See
//! `IPC-CONTRACT.md` §6. `device_list` matches that signature exactly (`() -> Vec<SerialDevice>`).
//! `device_set_preferred_port` predates `device_select` (`SPEC.md` §8 open question 21) and
//! is kept for any existing caller; `device_select` is the spec-correct command and also
//! captures `stickyHwid` (`FR-DEV-2`) — new frontend code should call that one.

use crate::commands::pipeline::resolved_pio;
use crate::commands::project::{entry_path, ProjectRegistryState};
use crate::commands::settings::SettingsState;
use crate::commands::util::{cache_dir, scratch_dir};
use crate::core::device::broker::{LeaseHolder, PortBroker};
use crate::core::device::hotplug;
use crate::core::device::list::{self, SerialDevice};
use crate::core::device::telemetry::{self, EsptoolEntrypointCache, Telemetry};
use crate::core::pio::boards;
use crate::core::project::registry;
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

/// `IPC-CONTRACT.md` §6: `(workspace, port) -> ()`. Sets `preferredPort` and, when the
/// chosen port's `hwid` yields a full VID:PID (+ optional serial), also captures
/// `stickyHwid` — `FR-DEV-2`'s "the app remembers the USB VID:PID + serial number... and
/// re-binds automatically when the same device reappears on a different port path."
#[tauri::command]
pub async fn device_select(
    app: AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
    port: String,
) -> Result<(), AppError> {
    let dir = {
        let reg = registry_state.0.lock().await;
        entry_path(&reg, &workspace)?
    };
    let mut settings = workspace::load(&dir)?.ok_or_else(|| AppError::Io {
        message: format!("{} has no .vibe/project.json", dir.display()),
    })?;

    settings.device.preferred_port = Some(port.clone());

    let pio_path = settings_state.0.lock().await.toolchain.pio_path.clone();
    if let Some(pio) = probes::resolve_tool(Tool::Pio, pio_path.as_deref(), &supervisor).await {
        let cwd = scratch_dir(&app)?;
        if let Ok(devices) = list::list_serial_devices(&supervisor, &pio, &cwd).await {
            if let Some(device) = devices.iter().find(|d| d.port == port) {
                if let Some(sticky) = hotplug::sticky_hwid_for(device) {
                    settings.device.sticky_hwid = Some(sticky);
                }
            }
        }
    }

    Ok(workspace::save(&dir, &settings)?)
}

/// `IPC-CONTRACT.md` §6: `(workspace, refresh) -> Telemetry`. Never returns an `AppError`
/// for "adapter didn't work" — `FR-DEV-3`: "never an error dialog, and never a blocked UI."
/// `refresh` is accepted for signature parity with `pipeline_targets`/`boards_list`'s
/// cache-bypass convention; telemetry itself is never cached (it reflects live hardware
/// state), so it has no effect here — `SPEC.md` §8 open question 28.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn device_telemetry(
    app: AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    broker: State<'_, PortBroker>,
    esptool_cache: State<'_, EsptoolEntrypointCache>,
    workspace: String,
    _refresh: bool,
) -> Result<Telemetry, AppError> {
    let dir = {
        let reg = registry_state.0.lock().await;
        entry_path(&reg, &workspace)?
    };
    let board_id = {
        let reg = registry_state.0.lock().await;
        registry::find(&reg, &workspace).and_then(|e| e.board_id.clone())
    };
    let port = workspace::load(&dir)?.and_then(|s| s.device.preferred_port);

    let pio = resolved_pio(&supervisor, &settings_state).await;

    let board = match (&pio, &board_id) {
        (Some(pio), Some(board_id)) => {
            let cache = cache_dir(&app)?;
            boards::find_board(&supervisor, pio, &dir, &cache, board_id).await.ok()
        }
        _ => None,
    };

    let is_esp = board.as_ref().map(|b| telemetry::is_esp_platform(&b.platform)).unwrap_or(false);

    // The port must be leased before probing it (`CLI-CONTRACT.md` §7.5) — but a busy port
    // degrades to "unavailable", never an error, matching every other telemetry failure mode.
    let lease = match &port {
        Some(p) if is_esp => match broker.acquire(p, LeaseHolder::Telemetry, false) {
            Ok(lease) => Some(lease),
            Err(busy) => {
                return Ok(Telemetry {
                    adapter: "esp".into(),
                    connected: false,
                    port: Some(p.clone()),
                    chip: None,
                    flash_size: None,
                    flash_vendor: None,
                    mac: None,
                    board,
                    unavailable_reason: Some(format!("Port is in use ({:?}).", busy.held_by)),
                });
            }
        },
        _ => None,
    };

    let result = telemetry::get_telemetry(&supervisor, pio.as_ref(), &dir, &esptool_cache, port.as_deref(), board).await;
    drop(lease);
    Ok(result)
}

// -----------------------------------------------------------------------------------------
// Hot-plug polling (`FR-DEV-1`, `FR-DEV-8`, `FR-DEV-9`). Not a `#[tauri::command]` — this is
// a background task started once in `lib.rs`'s `setup`, the same way
// `commands::pipeline::trigger_watch_build_if_applicable` is an orchestration function that
// lives in `commands/` despite not being invoked from the frontend directly.
// -----------------------------------------------------------------------------------------

/// The last-seen device list, so each poll only has to report what changed. Tauri-managed;
/// starts empty, which is indistinguishable from "polled once and found nothing" — the
/// first poll after startup will report every currently-attached device as `added`, which
/// is correct (nothing was known before it).
pub struct DeviceListState(pub tokio::sync::Mutex<Vec<SerialDevice>>);

/// Flipped by window focus/blur events to select the poll cadence — `FR-DEV-1`: "every 2s
/// while the app is focused, 10s in the background."
pub struct DeviceFocusState(pub std::sync::atomic::AtomicBool);

impl Default for DeviceFocusState {
    fn default() -> Self {
        Self(std::sync::atomic::AtomicBool::new(true))
    }
}

const FOCUSED_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
const BACKGROUND_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

/// `FR-DEV-2`: "re-binds automatically when the same device reappears on a different port
/// path." Scans every registered project's `.vibe/project.json` for a `stickyHwid` that
/// matches one of the newly-`added` devices, and — if that project's `preferredPort` isn't
/// already this port — updates and saves it. Runs for every workspace, not just the one
/// with an open Devices panel, so stickiness works even when nobody's looking at it.
async fn rebind_sticky_devices(app: &AppHandle, added: &[SerialDevice]) {
    use tauri::Manager;

    if added.is_empty() {
        return;
    }
    let registry_state = app.state::<ProjectRegistryState>();
    let entries = registry_state.0.lock().await.projects.clone();

    for entry in entries {
        let dir = std::path::PathBuf::from(&entry.path);
        let Ok(Some(mut settings)) = workspace::load(&dir) else {
            continue;
        };
        let Some(sticky) = settings.device.sticky_hwid.clone() else {
            continue;
        };
        if let Some(device) = added.iter().find(|d| hotplug::matches_sticky(d, &sticky)) {
            if settings.device.preferred_port.as_deref() != Some(device.port.as_str()) {
                settings.device.preferred_port = Some(device.port.clone());
                let _ = workspace::save(&dir, &settings);
            }
        }
    }
}

/// One enumeration pass: lists devices, diffs against the last-known list, and — only if
/// anything changed — emits `device://changed`. Also called directly (not just from the
/// poll loop) right after an upload finishes, per `FR-DEV-1`'s "immediately after any
/// upload" — ports commonly re-enumerate around a flash (`CLAUDE.md`'s landmine #10).
pub(crate) async fn poll_once_and_emit(app: &AppHandle) {
    use tauri::{Emitter, Manager};

    let settings_state = app.state::<SettingsState>();
    let supervisor = app.state::<ProcessSupervisor>();
    let Some(pio) = resolved_pio(&supervisor, &settings_state).await else {
        return;
    };
    let Ok(cwd) = scratch_dir(app) else {
        return;
    };
    let Ok(current) = list::list_serial_devices(&supervisor, &pio, &cwd).await else {
        return;
    };

    let list_state = app.state::<DeviceListState>();
    let mut known = list_state.0.lock().await;
    let diff = hotplug::diff_devices(&known, &current);
    if !diff.is_empty() {
        rebind_sticky_devices(app, &diff.added).await;
        let changed: hotplug::DeviceChanged = diff.into();
        let _ = app.emit("device://changed", changed);
    }
    *known = current;
}

/// `FR-DEV-1`'s "immediately after any upload," combined with `CLAUDE.md`'s landmine #10:
/// "boards re-enumerate after a flash — the port vanishes and returns after ~0.5-1.5s.
/// Retry with backoff before reporting a lost device." A single poll right after upload
/// would very likely catch the port mid-vanish and flicker a spurious disconnect,
/// immediately followed by the regular poll loop's own reconnect — so this retries
/// internally (without emitting each intermediate attempt) until the port count settles,
/// and only then diffs against what was known *before* the upload and emits once.
pub(crate) async fn poll_after_upload(app: &AppHandle) {
    use tauri::{Emitter, Manager};

    let settings_state = app.state::<SettingsState>();
    let supervisor = app.state::<ProcessSupervisor>();
    let Some(pio) = resolved_pio(&supervisor, &settings_state).await else {
        return;
    };
    let Ok(cwd) = scratch_dir(app) else {
        return;
    };

    let list_state = app.state::<DeviceListState>();
    let before = list_state.0.lock().await.clone();

    let Ok(mut current) = list::list_serial_devices(&supervisor, &pio, &cwd).await else {
        return;
    };
    // Spans the documented ~0.5-1.5s re-enumeration window with a few retries.
    for delay_ms in [300u64, 500, 700, 1000] {
        if hotplug::diff_devices(&before, &current).removed.is_empty() {
            break; // nothing looks disappeared — no need to keep waiting
        }
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        if let Ok(retried) = list::list_serial_devices(&supervisor, &pio, &cwd).await {
            current = retried;
        }
    }

    let mut known = list_state.0.lock().await;
    let diff = hotplug::diff_devices(&known, &current);
    if !diff.is_empty() {
        rebind_sticky_devices(app, &diff.added).await;
        let changed: hotplug::DeviceChanged = diff.into();
        let _ = app.emit("device://changed", changed);
    }
    *known = current;
}

/// Runs forever (spawned once at startup) — sleeps for the focus-dependent interval, then
/// polls. Deliberately polls *after* sleeping, not before, so app startup doesn't spawn
/// `pio` before the toolchain has necessarily been resolved once already by other startup
/// work.
pub(crate) async fn run_hotplug_poll_loop(app: AppHandle) {
    use tauri::Manager;

    loop {
        let focused = app
            .state::<DeviceFocusState>()
            .0
            .load(std::sync::atomic::Ordering::Relaxed);
        let interval = if focused { FOCUSED_POLL_INTERVAL } else { BACKGROUND_POLL_INTERVAL };
        tokio::time::sleep(interval).await;
        poll_once_and_emit(&app).await;
    }
}
