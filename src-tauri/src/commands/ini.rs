//! Thin Tauri command handlers for `platformio.ini` (`IPC-CONTRACT.md` §7). Logic lives in
//! `core::ini::*`; this just resolves paths/tools, spawns `pio`, and stitches the pieces
//! together.

use crate::commands::pipeline::resolved_pio;
use crate::commands::project::{entry_path, ProjectRegistryState};
use crate::commands::settings::{config_dir, SettingsState};
use crate::commands::util::{cache_dir, scratch_dir};
use crate::core::ini::document::{self, IniDocument};
use crate::core::ini::effective::{self, EffectiveConfig};
use crate::core::ini::lint::{self, LintReport};
use crate::core::ini::patch::{self, IniEdit};
use crate::core::ini::schema::{self, IniOptionSchema};
use crate::core::project::registry;
use crate::core::project::templates::{self, IniTemplate};
use crate::core::proc::{ProcKind, ProcessSupervisor, SpawnSpec};
use crate::core::toolchain::probes;
use crate::error::AppError;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tauri::{AppHandle, State};

fn ini_path(dir: &Path) -> PathBuf {
    dir.join("platformio.ini")
}

fn read_mtime_ms(path: &Path) -> Result<u64, AppError> {
    let meta = std::fs::metadata(path)?;
    let modified = meta.modified().map_err(|e| AppError::Io { message: e.to_string() })?;
    let ms = modified.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    Ok(ms)
}

fn read_ini_or_not_a_project(dir: &Path) -> Result<String, AppError> {
    let path = ini_path(dir);
    std::fs::read_to_string(&path).map_err(|_| AppError::NotAPioProject {
        path: dir.display().to_string(),
    })
}

/// Best-effort: a missing/unresolved `pio`, or a spawn failure, degrades to an empty
/// effective config rather than failing the whole read — the declared model (the file
/// itself) is always shown even if the "Effective" overlay can't be computed right now.
async fn fetch_effective_config(supervisor: &ProcessSupervisor, settings_state: &SettingsState, dir: &Path) -> EffectiveConfig {
    let Some(pio) = resolved_pio(supervisor, settings_state).await else {
        return EffectiveConfig::new();
    };
    let mut args = pio.extra_args.clone();
    args.extend(["project".into(), "config".into(), "-d".into(), dir.display().to_string(), "--json-output".into()]);
    let Ok(out) = supervisor
        .spawn_capture(SpawnSpec {
            program: pio.program.clone(),
            args,
            cwd: dir.to_path_buf(),
            env: vec![],
            kind: ProcKind::Pio,
            label: "pio-project-config-json".into(),
        })
        .await
    else {
        return EffectiveConfig::new();
    };
    if out.exit_code != 0 {
        return EffectiveConfig::new();
    }
    effective::parse_effective_config(&out.stdout).unwrap_or_default()
}

async fn read_document(supervisor: &ProcessSupervisor, settings_state: &SettingsState, dir: &Path) -> Result<IniDocument, AppError> {
    let raw = read_ini_or_not_a_project(dir)?;
    let mtime_ms = read_mtime_ms(&ini_path(dir))?;
    let effective = fetch_effective_config(supervisor, settings_state, dir).await;
    Ok(document::build_document(&raw, &effective, mtime_ms))
}

fn check_mtime(dir: &Path, expected_mtime_ms: u64) -> Result<(), AppError> {
    let actual = read_mtime_ms(&ini_path(dir))?;
    if actual != expected_mtime_ms {
        return Err(AppError::IniChangedOnDisk {
            path: ini_path(dir).display().to_string(),
        });
    }
    Ok(())
}

async fn workspace_dir(registry_state: &State<'_, ProjectRegistryState>, workspace: &str) -> Result<PathBuf, AppError> {
    let reg = registry_state.0.lock().await;
    entry_path(&reg, workspace)
}

#[tauri::command]
pub async fn ini_schema(
    app: AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
) -> Result<Vec<IniOptionSchema>, AppError> {
    let pio_path = settings_state.0.lock().await.toolchain.pio_path.clone();
    let pio = probes::resolve_tool(crate::core::toolchain::resolve::Tool::Pio, pio_path.as_deref(), &supervisor).await;
    let cwd = scratch_dir(&app)?;
    let cache = cache_dir(&app)?;

    let outcome = probes::probe_pio_core_dir(&supervisor, pio.as_ref(), &cwd).await;
    let (python_exe, core_version) = match &outcome.info {
        Some(info) => (info.python_exe.clone(), info.core_version.map(|v| v.to_string())),
        None => (None, None),
    };

    Ok(schema::get_schema(&supervisor, python_exe.as_deref(), core_version.as_deref(), &cwd, &cache).await)
}

#[tauri::command]
pub async fn ini_read(
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
) -> Result<IniDocument, AppError> {
    let dir = workspace_dir(&registry_state, &workspace).await?;
    read_document(&supervisor, &settings_state, &dir).await
}

#[tauri::command]
pub async fn ini_apply(
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
    edits: Vec<IniEdit>,
    expected_mtime_ms: u64,
) -> Result<IniDocument, AppError> {
    let dir = workspace_dir(&registry_state, &workspace).await?;
    check_mtime(&dir, expected_mtime_ms)?;
    let raw = read_ini_or_not_a_project(&dir)?;
    let new_raw = patch::apply_edits(&raw, &edits);
    std::fs::write(ini_path(&dir), &new_raw)?;
    read_document(&supervisor, &settings_state, &dir).await
}

#[tauri::command]
pub async fn ini_write_raw(
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
    raw: String,
    expected_mtime_ms: u64,
) -> Result<IniDocument, AppError> {
    let dir = workspace_dir(&registry_state, &workspace).await?;
    check_mtime(&dir, expected_mtime_ms)?;
    std::fs::write(ini_path(&dir), &raw)?;
    read_document(&supervisor, &settings_state, &dir).await
}

/// `FR-INI-5`: `pio project config --lint` **without** `--json-output` — the JSON form
/// emits a Python repr, not JSON (`CLI-CONTRACT.md` §4.3).
#[tauri::command]
pub async fn ini_lint(
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
) -> Result<LintReport, AppError> {
    let dir = workspace_dir(&registry_state, &workspace).await?;
    let pio = resolved_pio(&supervisor, &settings_state).await.ok_or_else(|| AppError::ToolMissing {
        tool: "pio".into(),
        install_action: true,
    })?;
    let mut args = pio.extra_args.clone();
    args.extend(["project".into(), "config".into(), "-d".into(), dir.display().to_string(), "--lint".into()]);
    let out = supervisor
        .spawn_capture(SpawnSpec {
            program: pio.program.clone(),
            args,
            cwd: dir.clone(),
            env: vec![],
            kind: ProcKind::Pio,
            label: "pio-project-config-lint".into(),
        })
        .await?;
    Ok(lint::parse_lint_output(&format!("{}{}", out.stdout, out.stderr)))
}

#[tauri::command]
pub async fn ini_template_save(
    app: AppHandle,
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
    name: String,
) -> Result<(), AppError> {
    let dir = workspace_dir(&registry_state, &workspace).await?;
    let raw = read_ini_or_not_a_project(&dir)?;
    let board_id = {
        let reg = registry_state.0.lock().await;
        registry::find(&reg, &workspace).and_then(|e| e.board_id.clone())
    };
    let claude_md = std::fs::read_to_string(dir.join("CLAUDE.md")).ok();
    let config = config_dir(&app)?;
    templates::save(&config, &name, raw, board_id, claude_md)?;
    Ok(())
}

#[tauri::command]
pub async fn ini_template_list(app: AppHandle) -> Result<Vec<IniTemplate>, AppError> {
    let config = config_dir(&app)?;
    Ok(templates::list(&config))
}

#[tauri::command]
pub async fn ini_template_apply(
    app: AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
    name: String,
) -> Result<IniDocument, AppError> {
    let dir = workspace_dir(&registry_state, &workspace).await?;
    let config = config_dir(&app)?;
    let template = templates::find_by_name(&config, &name).ok_or_else(|| AppError::Io {
        message: format!("no template named {name}"),
    })?;
    let raw = read_ini_or_not_a_project(&dir)?;
    let edits = templates::merge_edits(&template);
    let new_raw = patch::apply_edits(&raw, &edits);
    std::fs::write(ini_path(&dir), &new_raw)?;
    read_document(&supervisor, &settings_state, &dir).await
}
