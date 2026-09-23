//! Thin Tauri command handlers for libraries/packages and PlatformIO's own global settings
//! (`IPC-CONTRACT.md` §7). Logic lives in `core::pio::{packages,registry,global_settings}`.

use crate::commands::doctor::HttpClientState;
use crate::commands::pipeline::{resolved_pio, workspace_dir_and_env};
use crate::commands::project::ProjectRegistryState;
use crate::commands::settings::SettingsState;
use crate::core::pio::global_settings::{self, PioSetting};
use crate::core::pio::packages::{self, InstalledPackage, OutdatedPackage, PkgKind};
use crate::core::pio::registry::{self, PackagePage};
use crate::core::proc::events::{spawn_with_proc_events, ProcEvent};
use crate::core::proc::{ProcId, ProcKind, ProcessSupervisor, SpawnSpec};
use crate::error::AppError;
use tauri::ipc::Channel;
use tauri::State;

#[tauri::command]
pub async fn pkg_search(
    http_client: State<'_, HttpClientState>,
    query: String,
    qualifiers: Vec<(String, String)>,
    page: u32,
    sort: Option<String>,
) -> Result<PackagePage, AppError> {
    let full_query = registry::build_query(&query, &qualifiers);
    registry::search(&http_client.0, &full_query, page.max(1), sort.as_deref()).await
}

async fn spawn_pkg_command(
    supervisor: &ProcessSupervisor,
    settings_state: &SettingsState,
    dir: std::path::PathBuf,
    args: Vec<String>,
    label: &str,
    on_event: Channel<ProcEvent>,
) -> Result<ProcId, AppError> {
    let pio = resolved_pio(supervisor, settings_state).await.ok_or_else(|| AppError::ToolMissing {
        tool: "pio".into(),
        install_action: true,
    })?;
    let mut full_args = pio.extra_args.clone();
    full_args.extend(args);
    let spec = SpawnSpec {
        program: pio.program.clone(),
        args: full_args,
        cwd: dir,
        env: vec![],
        kind: ProcKind::Pio,
        label: label.into(),
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let proc_id = spawn_with_proc_events(supervisor, spec, tx).await?;
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let _ = on_event.send(ev);
        }
    });
    Ok(proc_id)
}

/// `FR-INI-8`: installing writes `lib_deps` for you (`pio pkg install`'s own behavior,
/// unless `--no-save` is passed — never passed here) — this command does **not** also
/// patch the ini itself, avoiding a double entry (`CLI-CONTRACT.md` §7.2).
#[tauri::command]
pub async fn pkg_install(
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
    spec: String,
    kind: PkgKind,
    on_event: Channel<ProcEvent>,
) -> Result<ProcId, AppError> {
    let (dir, env) = workspace_dir_and_env(&registry_state, &workspace).await?;
    let args = packages::install_args(&dir, &env, kind, &spec);
    spawn_pkg_command(&supervisor, &settings_state, dir, args, "pio-pkg-install", on_event).await
}

#[tauri::command]
pub async fn pkg_uninstall(
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
    spec: String,
    kind: PkgKind,
    on_event: Channel<ProcEvent>,
) -> Result<ProcId, AppError> {
    let (dir, env) = workspace_dir_and_env(&registry_state, &workspace).await?;
    let args = packages::uninstall_args(&dir, &env, kind, &spec);
    spawn_pkg_command(&supervisor, &settings_state, dir, args, "pio-pkg-uninstall", on_event).await
}

#[tauri::command]
pub async fn pkg_installed(
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
) -> Result<Vec<InstalledPackage>, AppError> {
    let (dir, env) = workspace_dir_and_env(&registry_state, &workspace).await?;
    let pio = resolved_pio(&supervisor, &settings_state).await.ok_or_else(|| AppError::ToolMissing {
        tool: "pio".into(),
        install_action: true,
    })?;
    let mut args = pio.extra_args.clone();
    args.extend(packages::list_args(&dir, &env));
    let out = supervisor
        .spawn_capture(SpawnSpec {
            program: pio.program.clone(),
            args,
            cwd: dir,
            env: vec![],
            kind: ProcKind::Pio,
            label: "pio-pkg-list".into(),
        })
        .await?;
    Ok(packages::parse_pkg_list(&out.stdout))
}

#[tauri::command]
pub async fn pkg_outdated(
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
) -> Result<Vec<OutdatedPackage>, AppError> {
    let (dir, env) = workspace_dir_and_env(&registry_state, &workspace).await?;
    let pio = resolved_pio(&supervisor, &settings_state).await.ok_or_else(|| AppError::ToolMissing {
        tool: "pio".into(),
        install_action: true,
    })?;
    let mut args = pio.extra_args.clone();
    args.extend(packages::outdated_args(&dir, &env));
    let out = supervisor
        .spawn_capture(SpawnSpec {
            program: pio.program.clone(),
            args,
            cwd: dir,
            env: vec![],
            kind: ProcKind::Pio,
            label: "pio-pkg-outdated".into(),
        })
        .await?;
    Ok(packages::parse_pkg_outdated(&out.stdout))
}

#[tauri::command]
pub async fn pio_settings_get(app: tauri::AppHandle, supervisor: State<'_, ProcessSupervisor>, settings_state: State<'_, SettingsState>) -> Result<Vec<PioSetting>, AppError> {
    let pio = resolved_pio(&supervisor, &settings_state).await.ok_or_else(|| AppError::ToolMissing {
        tool: "pio".into(),
        install_action: true,
    })?;
    let cwd = crate::commands::util::scratch_dir(&app)?;
    let mut args = pio.extra_args.clone();
    args.extend(global_settings::get_args());
    let out = supervisor
        .spawn_capture(SpawnSpec {
            program: pio.program.clone(),
            args,
            cwd,
            env: vec![],
            kind: ProcKind::Pio,
            label: "pio-settings-get".into(),
        })
        .await?;
    Ok(global_settings::parse_settings_get(&out.stdout))
}

#[tauri::command]
pub async fn pio_settings_set(
    app: tauri::AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    name: String,
    value: String,
) -> Result<Vec<PioSetting>, AppError> {
    let pio = resolved_pio(&supervisor, &settings_state).await.ok_or_else(|| AppError::ToolMissing {
        tool: "pio".into(),
        install_action: true,
    })?;
    let cwd = crate::commands::util::scratch_dir(&app)?;
    let mut args = pio.extra_args.clone();
    args.extend(global_settings::set_args(&name, &value));
    supervisor
        .spawn_capture(SpawnSpec {
            program: pio.program.clone(),
            args,
            cwd: cwd.clone(),
            env: vec![],
            kind: ProcKind::Pio,
            label: "pio-settings-set".into(),
        })
        .await?;

    let mut get_args_full = pio.extra_args.clone();
    get_args_full.extend(global_settings::get_args());
    let out = supervisor
        .spawn_capture(SpawnSpec {
            program: pio.program.clone(),
            args: get_args_full,
            cwd,
            env: vec![],
            kind: ProcKind::Pio,
            label: "pio-settings-get".into(),
        })
        .await?;
    Ok(global_settings::parse_settings_get(&out.stdout))
}

#[tauri::command]
pub async fn pio_system_prune(
    app: tauri::AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    on_event: Channel<ProcEvent>,
) -> Result<ProcId, AppError> {
    let pio = resolved_pio(&supervisor, &settings_state).await.ok_or_else(|| AppError::ToolMissing {
        tool: "pio".into(),
        install_action: true,
    })?;
    let cwd = crate::commands::util::scratch_dir(&app)?;
    let mut args = pio.extra_args.clone();
    args.extend(["system".into(), "prune".into()]);
    let spec = SpawnSpec {
        program: pio.program.clone(),
        args,
        cwd,
        env: vec![],
        kind: ProcKind::Pio,
        label: "pio-system-prune".into(),
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let proc_id = spawn_with_proc_events(&supervisor, spec, tx).await?;
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let _ = on_event.send(ev);
        }
    });
    Ok(proc_id)
}
