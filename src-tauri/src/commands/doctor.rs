//! Thin Tauri command handlers for Doctor & toolchain. See `IPC-CONTRACT.md` §2. All the
//! actual logic (probing, resolution, install streaming, diagnostics bundling) lives in
//! `core::toolchain::*` and is unit/integration-tested there without Tauri.

use crate::commands::settings::SettingsState;
use crate::commands::util::{cache_dir, scratch_dir};
use crate::core::proc::{ProcId, ProcessSupervisor};
use crate::core::toolchain::doctor::{run_doctor, DoctorCache, DoctorContext, FOCUS_REPROBE_AGE};
use crate::core::toolchain::install::{self, InstallEvent};
use crate::core::toolchain::resolve::{self, Platform, Tool};
use crate::core::toolchain::types::{DoctorReport, ProbeResult, RemediationKind};
use crate::core::toolchain::{diagnostics, probes};
use crate::error::AppError;
use serde::Deserialize;
use std::path::PathBuf;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex;

pub struct DoctorState(pub Mutex<DoctorCache>);
pub struct HttpClientState(pub reqwest::Client);

async fn probe(
    app: &AppHandle,
    supervisor: &ProcessSupervisor,
    settings: &SettingsState,
    http_client: &reqwest::Client,
) -> Result<DoctorReport, AppError> {
    let scratch = scratch_dir(app)?;
    let toolchain = settings.0.lock().await.toolchain.clone();
    let ctx = DoctorContext {
        supervisor,
        settings: &toolchain,
        scratch_dir: &scratch,
        http_client,
        disable_udev_rules_check: false,
        network_probe_url: probes::REGISTRY_PROBE_URL,
    };
    Ok(run_doctor(ctx).await)
}

#[tauri::command]
pub async fn doctor_run(
    app: AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    doctor_state: State<'_, DoctorState>,
    settings_state: State<'_, SettingsState>,
    http: State<'_, HttpClientState>,
    force: bool,
) -> Result<DoctorReport, AppError> {
    if !force {
        let cache = doctor_state.0.lock().await;
        if let (Some(report), false) = (cache.get(), cache.is_stale(FOCUS_REPROBE_AGE)) {
            return Ok(report.clone());
        }
    }

    let report = probe(&app, &supervisor, &settings_state, &http.0).await?;
    doctor_state.0.lock().await.set(report.clone());
    Ok(report)
}

#[tauri::command]
pub async fn doctor_get_cached(doctor_state: State<'_, DoctorState>) -> Result<Option<DoctorReport>, AppError> {
    Ok(doctor_state.0.lock().await.get().cloned())
}

#[tauri::command]
pub async fn toolchain_install(
    app: AppHandle,
    settings_state: State<'_, SettingsState>,
    kind: RemediationKind,
    on_event: Channel<InstallEvent>,
) -> Result<ProcId, AppError> {
    let op_id = ProcId::new();
    let scratch = scratch_dir(&app)?.join(op_id.0.clone());
    let platform = resolve::current_platform();
    let toolchain_settings = settings_state.0.lock().await.toolchain.clone();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    // Forwards every event to the frontend as it arrives; deliberately not awaited — it
    // runs for the lifetime of the mpsc channel, which the installer task below closes by
    // dropping `tx` when it finishes.
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let _ = on_event.send(ev);
        }
    });

    // `AppHandle` is `Clone` + `'static`; each spawned task re-resolves the managed
    // `ProcessSupervisor` through it rather than trying to move a borrowed `State` across
    // the `tokio::spawn` boundary.
    let install_app = app.clone();
    match kind {
        RemediationKind::InstallClaude => {
            tokio::spawn(async move {
                let supervisor = install_app.state::<ProcessSupervisor>();
                let tools = match install::resolve_system_tools(platform) {
                    Ok(t) => t,
                    Err(e) => {
                        let _ = tx.send(InstallEvent::Finished { success: false, exit_code: -1 });
                        tracing::warn!("claude install: {e}");
                        return;
                    }
                };
                let _ = install::run_claude_install(&supervisor, platform, &tools, &scratch, tx).await;
            });
        }
        RemediationKind::InstallPio => {
            tokio::spawn(async move {
                let supervisor = install_app.state::<ProcessSupervisor>();
                let python_resolution =
                    probes::resolve_tool(Tool::Python, toolchain_settings.python_path.as_deref(), &supervisor).await;
                let Some(python) = python_resolution else {
                    let _ = tx.send(InstallEvent::Finished { success: false, exit_code: -1 });
                    return;
                };
                let curl = match which::which("curl") {
                    Ok(c) => c,
                    Err(_) => {
                        let _ = tx.send(InstallEvent::Finished { success: false, exit_code: -1 });
                        return;
                    }
                };
                let _ = install::run_pio_install(
                    &supervisor,
                    &curl,
                    python.program,
                    python.extra_args,
                    &scratch,
                    tx,
                )
                .await;
            });
        }
        RemediationKind::AuthenticateClaude
        | RemediationKind::InstallUdevRules
        | RemediationKind::OpenUrl
        | RemediationKind::ShowCommand => {
            let _ = tx.send(InstallEvent::Finished { success: false, exit_code: -1 });
            tracing::warn!("toolchain_install called with a non-installer remediation kind");
        }
    }

    Ok(op_id)
}

#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum ToolchainTool {
    Claude,
    Pio,
}

#[tauri::command]
pub async fn toolchain_set_path(
    app: AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    tool: ToolchainTool,
    path: String,
) -> Result<ProbeResult, AppError> {
    {
        let mut settings = settings_state.0.lock().await;
        match tool {
            ToolchainTool::Claude => settings.toolchain.claude_path = Some(path.clone()),
            ToolchainTool::Pio => settings.toolchain.pio_path = Some(path.clone()),
        }
        let dir = crate::commands::settings::config_dir(&app)?;
        crate::core::settings::save(&dir, &settings)?;
    }

    let scratch = scratch_dir(&app)?;
    let resolution = resolve::resolve_full(
        match tool {
            ToolchainTool::Claude => Tool::Claude,
            ToolchainTool::Pio => Tool::Pio,
        },
        resolve::current_platform(),
        Some(std::path::Path::new(&path)),
        &resolve::home_dir().unwrap_or_else(|| PathBuf::from(".")),
        &std::collections::HashMap::new(),
        None,
        &supervisor,
    )
    .await;

    Ok(match tool {
        ToolchainTool::Claude => probes::probe_claude_binary(&supervisor, resolution.as_ref(), &scratch).await,
        ToolchainTool::Pio => probes::probe_pio_binary(&supervisor, resolution.as_ref(), &scratch).await,
    })
}

#[tauri::command]
pub async fn toolchain_open_auth_terminal(
    app: AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
) -> Result<(), AppError> {
    let toolchain_settings = settings_state.0.lock().await.toolchain.clone();
    let resolution = probes::resolve_tool(Tool::Claude, toolchain_settings.claude_path.as_deref(), &supervisor)
        .await
        .ok_or(AppError::ToolMissing {
            tool: "claude".into(),
            install_action: true,
        })?;

    let (program, args): (PathBuf, Vec<String>) = match resolve::current_platform() {
        Platform::MacOs => (
            PathBuf::from("/usr/bin/open"),
            vec!["-a".into(), "Terminal".into(), resolution.program.display().to_string()],
        ),
        Platform::Windows => (
            which::which("cmd.exe").map_err(|_| AppError::ToolMissing {
                tool: "cmd.exe".into(),
                install_action: false,
            })?,
            vec![
                "/c".into(),
                "start".into(),
                "".into(),
                "cmd".into(),
                "/k".into(),
                resolution.program.display().to_string(),
            ],
        ),
        Platform::Linux => {
            let candidates = [
                "x-terminal-emulator",
                "gnome-terminal",
                "konsole",
                "xfce4-terminal",
                "alacritty",
                "kitty",
            ];
            let found = candidates.iter().find_map(|c| which::which(c).ok());
            let terminal = found.ok_or_else(|| AppError::Io {
                message: format!(
                    "no terminal emulator found; run {} yourself to sign in",
                    resolution.program.display()
                ),
            })?;
            (terminal, vec!["-e".into(), resolution.program.display().to_string()])
        }
    };

    let scratch = scratch_dir(&app)?;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    supervisor
        .spawn_streaming(
            crate::core::proc::SpawnSpec {
                program,
                args,
                cwd: scratch,
                env: vec![],
                kind: crate::core::proc::ProcKind::Tool,
                label: "open-auth-terminal".into(),
            },
            tx,
        )
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn diagnostics_export(
    app: AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    doctor_state: State<'_, DoctorState>,
    settings_state: State<'_, SettingsState>,
    http: State<'_, HttpClientState>,
) -> Result<String, AppError> {
    let report = {
        let cache = doctor_state.0.lock().await;
        cache.get().cloned()
    };
    let report = match report {
        Some(r) => r,
        None => {
            let r = probe(&app, &supervisor, &settings_state, &http.0).await?;
            doctor_state.0.lock().await.set(r.clone());
            r
        }
    };

    let settings = settings_state.0.lock().await.clone();
    let scratch = scratch_dir(&app)?;

    let toolchain_settings = settings.toolchain.clone();
    let pio_resolution = probes::resolve_tool(Tool::Pio, toolchain_settings.pio_path.as_deref(), &supervisor).await;
    let claude_resolution =
        probes::resolve_tool(Tool::Claude, toolchain_settings.claude_path.as_deref(), &supervisor).await;

    let pio_system_info_json = match &pio_resolution {
        Some(r) => spawn_capture_text(&supervisor, r, &["system", "info", "--json-output"], &scratch).await,
        None => None,
    };
    let pio_settings_text = match &pio_resolution {
        Some(r) => spawn_capture_text(&supervisor, r, &["settings", "get"], &scratch).await,
        None => None,
    };
    let claude_doctor_text = match &claude_resolution {
        Some(r) => spawn_capture_text(&supervisor, r, &["doctor"], &scratch).await,
        None => None,
    };

    let dest_dir = cache_dir(&app)?;
    let home = resolve::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let app_version = app.package_info().version.to_string();

    let inputs = diagnostics::DiagnosticsInputs {
        doctor_report: &report,
        global_settings: &settings,
        pio_system_info_json: pio_system_info_json.as_deref(),
        pio_settings_text: pio_settings_text.as_deref(),
        claude_doctor_text: claude_doctor_text.as_deref(),
        app_log: None, // wired once the log sink's file path is exposed (M9)
        active_platformio_ini: None, // no workspace concept until M2
        last_build_log: None,        // no build pipeline until M5
        app_version: &app_version,
        git_sha: None,
        redact_serials: false,
    };

    let path = diagnostics::build_bundle(&inputs, &home, &dest_dir)?;
    Ok(path.display().to_string())
}

async fn spawn_capture_text(
    supervisor: &ProcessSupervisor,
    resolution: &resolve::Resolution,
    args: &[&str],
    cwd: &std::path::Path,
) -> Option<String> {
    let mut full_args = resolution.extra_args.clone();
    full_args.extend(args.iter().map(|s| s.to_string()));
    let spec = crate::core::proc::SpawnSpec {
        program: resolution.program.clone(),
        args: full_args,
        cwd: cwd.to_path_buf(),
        env: vec![],
        kind: crate::core::proc::ProcKind::Tool,
        label: "diagnostics-capture".into(),
    };
    supervisor.spawn_capture(spec).await.ok().map(|o| o.stdout)
}
