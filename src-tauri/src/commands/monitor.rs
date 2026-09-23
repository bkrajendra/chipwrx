//! Thin Tauri command handlers for the in-app serial monitor (`IPC-CONTRACT.md` §6).
//! Logic lives in `core::device::monitor`; port ownership goes through `core::device::broker`.

use crate::commands::pipeline::{resolved_pio, workspace_dir_and_env};
use crate::commands::project::ProjectRegistryState;
use crate::commands::settings::SettingsState;
use crate::core::device::broker::{Lease, LeaseHolder, PortBroker};
use crate::core::device::monitor::{self, MonitorEvent, RingBuffer, SerialMonitorSettings};
use crate::core::project::workspace;
use crate::core::proc::ProcId;
use crate::core::toolchain::resolve::{self, Platform};
use crate::core::proc::{ProcKind, ProcessSupervisor, SpawnSpec};
use crate::error::AppError;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex;

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct MonitorSession {
    /// Holds the `PortBroker` lease alive; dropping this (via `sessions.remove`) frees the
    /// port immediately.
    _lease: Lease,
    cmd_tx: std::sync::mpsc::Sender<monitor::MonitorCommand>,
    log: RingBuffer,
    settings: SerialMonitorSettings,
    on_event: Channel<MonitorEvent>,
}

pub struct MonitorState(Mutex<HashMap<String, MonitorSession>>);

impl MonitorState {
    pub fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }

    /// `FR-DEV-4`: "If Upload is pressed while the monitor holds the port, the broker (a)
    /// stops the monitor... and the UI narrates each step." Same applies to `pio test`
    /// (`M9`, `CLI-CONTRACT.md` §5.3) — `by` names whichever one is preempting, for the
    /// narration. If a monitor is running for `workspace`, signals it to stop (which also
    /// drops its `Lease`, freeing the port) and returns the channel it was streaming events
    /// on, so the caller can restart it identically afterward and keep narrating on the
    /// same channel the frontend is already listening to.
    pub(crate) async fn preempt(&self, workspace: &str, by: &str) -> Option<Channel<MonitorEvent>> {
        let mut sessions = self.0.lock().await;
        let session = sessions.remove(workspace)?;
        let _ = session.on_event.send(MonitorEvent::Preempted { by: by.into() });
        let _ = session.cmd_tx.send(monitor::MonitorCommand::Stop);
        Some(session.on_event)
    }
}

impl Default for MonitorState {
    fn default() -> Self {
        Self::new()
    }
}

async fn read_ini(dir: &std::path::Path) -> String {
    tokio::fs::read_to_string(dir.join("platformio.ini")).await.unwrap_or_default()
}

/// Opens the port and spawns the I/O thread, then registers the session and emits
/// `Opened`. Shared by `monitor_start` and the upload-preemption reattach path
/// (`FR-DEV-4`) so both go through identical setup.
pub(crate) async fn start_session(
    app: &AppHandle,
    dir: &std::path::Path,
    env_name: &str,
    workspace: &str,
    port: &str,
    lease: Lease,
    on_event: &Channel<MonitorEvent>,
) -> Result<(), AppError> {
    let ini = read_ini(dir).await;
    let settings = monitor::settings_from_ini(&ini, env_name);

    let port_owned = port.to_string();
    let settings_for_open = settings.clone();
    let opened = tokio::task::spawn_blocking(move || monitor::open_port(&port_owned, &settings_for_open))
        .await
        .map_err(|e| AppError::Io {
            message: format!("monitor open task panicked: {e}"),
        })??;

    let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel::<monitor::EngineEvent>();
    let cmd_tx = monitor::spawn_io_thread(opened, events_tx);

    {
        let monitor_state = app.state::<MonitorState>();
        let mut sessions = monitor_state.0.lock().await;
        sessions.insert(
            workspace.to_string(),
            MonitorSession {
                _lease: lease,
                cmd_tx,
                log: RingBuffer::default(),
                settings: settings.clone(),
                on_event: on_event.clone(),
            },
        );
    }

    let _ = on_event.send(MonitorEvent::Opened {
        port: port.to_string(),
        baud: settings.baud,
    });

    let app_for_task = app.clone();
    let ws_for_task = workspace.to_string();
    let on_event = on_event.clone();
    tokio::spawn(async move {
        while let Some(ev) = events_rx.recv().await {
            match ev {
                monitor::EngineEvent::Data(bytes) => {
                    {
                        let monitor_state = app_for_task.state::<MonitorState>();
                        let mut sessions = monitor_state.0.lock().await;
                        if let Some(session) = sessions.get_mut(&ws_for_task) {
                            session.log.push(&bytes);
                        }
                    }
                    let chunk = String::from_utf8_lossy(&bytes).into_owned();
                    let _ = on_event.send(MonitorEvent::Data { chunk, ts_ms: epoch_ms() });
                }
                monitor::EngineEvent::Error(message) => {
                    let _ = on_event.send(MonitorEvent::Error {
                        error: AppError::Io { message },
                    });
                }
                monitor::EngineEvent::Closed => {
                    let _ = on_event.send(MonitorEvent::Closed { reason: "port closed".into() });
                    break;
                }
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn monitor_start(
    app: AppHandle,
    registry_state: State<'_, ProjectRegistryState>,
    broker: State<'_, PortBroker>,
    monitor_state: State<'_, MonitorState>,
    workspace: String,
    on_event: Channel<MonitorEvent>,
) -> Result<ProcId, AppError> {
    let (dir, env) = workspace_dir_and_env(&registry_state, &workspace).await?;
    let port = workspace::load(&dir)?
        .and_then(|s| s.device.preferred_port)
        .ok_or_else(|| AppError::Io {
            message: "no device selected for this workspace".into(),
        })?;

    // Restarting an already-running monitor is idempotent: stop the old session first.
    {
        let mut sessions = monitor_state.0.lock().await;
        if let Some(old) = sessions.remove(&workspace) {
            let _ = old.cmd_tx.send(monitor::MonitorCommand::Stop);
        }
    }

    let lease = broker.acquire(&port, LeaseHolder::Monitor, false).map_err(|busy| AppError::PortBusy {
        port: port.clone(),
        held_by: format!("{:?}", busy.held_by),
    })?;

    start_session(&app, &dir, &env, &workspace, &port, lease, &on_event).await?;
    Ok(ProcId::new())
}

#[tauri::command]
pub async fn monitor_stop(monitor_state: State<'_, MonitorState>, workspace: String) -> Result<(), AppError> {
    let mut sessions = monitor_state.0.lock().await;
    if let Some(session) = sessions.remove(&workspace) {
        let _ = session.cmd_tx.send(monitor::MonitorCommand::Stop);
        // Dropping `session` here also drops its `Lease`, freeing the port right away.
    }
    Ok(())
}

#[tauri::command]
pub async fn monitor_send(monitor_state: State<'_, MonitorState>, workspace: String, text: String) -> Result<(), AppError> {
    let sessions = monitor_state.0.lock().await;
    let session = sessions.get(&workspace).ok_or_else(|| AppError::Io {
        message: "monitor is not running for this workspace".into(),
    })?;
    let mut bytes = text.into_bytes();
    bytes.extend_from_slice(monitor::eol_bytes(&session.settings.eol));
    session
        .cmd_tx
        .send(monitor::MonitorCommand::Send(bytes))
        .map_err(|_| AppError::Io {
            message: "monitor's I/O thread has already exited".into(),
        })
}

#[tauri::command]
pub async fn monitor_save_log(monitor_state: State<'_, MonitorState>, workspace: String, path: String) -> Result<(), AppError> {
    let bytes = {
        let sessions = monitor_state.0.lock().await;
        let session = sessions.get(&workspace).ok_or_else(|| AppError::Io {
            message: "monitor is not running for this workspace".into(),
        })?;
        session.log.as_bytes()
    };
    let write = tokio::task::spawn_blocking(move || std::fs::write(&path, &bytes))
        .await
        .map_err(|e| AppError::Io {
            message: format!("save-log task panicked: {e}"),
        })?;
    write.map_err(AppError::from)
}

/// `FR-DEV-6`: launches `pio device monitor -d <dir> -e <env>` in the platform's terminal.
/// "The lease is released first" — if this workspace's own in-app monitor is running, it's
/// stopped before the external one starts, same as `monitor_stop`.
///
/// Only Windows is implemented here — CLI-CONTRACT.md documents the `pio device monitor`
/// invocation itself but not a platform terminal-launch mechanism for any OS, and this dev
/// environment is Windows-only, so macOS/Linux aren't guessed at. `SPEC.md` §8 open
/// question 29.
#[tauri::command]
pub async fn monitor_open_external(
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    monitor_state: State<'_, MonitorState>,
    workspace: String,
) -> Result<(), AppError> {
    {
        let mut sessions = monitor_state.0.lock().await;
        if let Some(session) = sessions.remove(&workspace) {
            let _ = session.cmd_tx.send(monitor::MonitorCommand::Stop);
        }
    }

    let (dir, env) = workspace_dir_and_env(&registry_state, &workspace).await?;
    let pio = resolved_pio(&supervisor, &settings_state).await.ok_or_else(|| AppError::ToolMissing {
        tool: "pio".into(),
        install_action: true,
    })?;

    let platform = resolve::current_platform();
    if platform != Platform::Windows {
        return Err(AppError::Io {
            message: "Opening an external monitor terminal isn't implemented on this platform yet — use the in-app monitor.".into(),
        });
    }

    let cmd_exe = PathBuf::from(std::env::var("COMSPEC").unwrap_or_else(|_| "C:\\Windows\\System32\\cmd.exe".into()));
    let mut pio_args = pio.extra_args.clone();
    pio_args.extend(["device".into(), "monitor".into(), "-d".into(), dir.display().to_string(), "-e".into(), env]);

    let mut args = vec!["/c".to_string(), "start".to_string(), String::new(), pio.program.display().to_string()];
    args.extend(pio_args);

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    supervisor
        .spawn_streaming(
            SpawnSpec {
                program: cmd_exe,
                args,
                cwd: dir,
                env: vec![],
                kind: ProcKind::Tool,
                label: "pio-device-monitor-external".into(),
            },
            tx,
        )
        .await?;
    Ok(())
}
