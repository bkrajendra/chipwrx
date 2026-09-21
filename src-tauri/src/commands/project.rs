//! Thin Tauri command handlers for projects. See `IPC-CONTRACT.md` §3. Logic lives in
//! `core::pio::{boards,init}` and `core::project::*`.

use crate::commands::settings::SettingsState;
use crate::commands::util::{cache_dir, scratch_dir};
use crate::core::pio::boards::{self, BoardBrief};
use crate::core::pio::init::{self, CreateProjectRequest};
use crate::core::proc::events::{spawn_with_proc_events, ProcEvent};
use crate::core::proc::{ProcId, ProcKind, ProcessSupervisor, SpawnSpec};
use crate::core::project::types::{CreatedBy, ProjectEntry, ProjectRegistry, TrustScan};
use crate::core::project::{claude_md, editor, env as project_env, registry, trust, workspace};
use crate::core::toolchain::probes;
use crate::core::toolchain::resolve::{self, Tool};
use crate::error::AppError;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::Mutex;

pub struct ProjectRegistryState(pub Mutex<ProjectRegistry>);

pub fn load_registry_at_startup(app: &AppHandle) -> Result<ProjectRegistry, AppError> {
    let dir = crate::commands::settings::config_dir(app)?;
    let mut reg = registry::load(&dir)?;

    // Self-heal entries a pre-fix build of this app wrote with a Windows `\\?\`
    // extended-length prefix still in `path` (ugly in the UI and not reliably accepted by
    // every external program's argv parsing — see `workspace::canonicalize_workspace`).
    let mut changed = false;
    for entry in &mut reg.projects {
        let cleaned = workspace::strip_windows_verbatim_prefix_str(&entry.path);
        if cleaned != entry.path {
            entry.path = cleaned;
            changed = true;
        }
    }
    if changed {
        let _ = registry::save(&dir, &reg);
    }

    Ok(reg)
}

async fn resolved_pio(
    supervisor: &ProcessSupervisor,
    settings: &SettingsState,
) -> Option<resolve::Resolution> {
    let pio_path = settings.0.lock().await.toolchain.pio_path.clone();
    probes::resolve_tool(Tool::Pio, pio_path.as_deref(), supervisor).await
}

pub(crate) fn entry_path(registry: &ProjectRegistry, id: &str) -> Result<PathBuf, AppError> {
    registry::find(registry, id)
        .map(|e| PathBuf::from(&e.path))
        .ok_or_else(|| AppError::Io {
            message: format!("no project registered with id {id}"),
        })
}

// ---------------------------------------------------------------------------------------
// boards_list
// ---------------------------------------------------------------------------------------

#[tauri::command]
pub async fn boards_list(
    app: AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    refresh: bool,
    installed_only: bool,
) -> Result<Vec<BoardBrief>, AppError> {
    let pio = resolved_pio(&supervisor, &settings_state).await;
    let ttl_hours = settings_state.0.lock().await.network.board_catalogue_ttl_hours;
    let cwd = scratch_dir(&app)?;
    let cache = cache_dir(&app)?;

    let (boards, _status) = boards::get_boards(
        &supervisor,
        pio.as_ref(),
        &cwd,
        &cache,
        Duration::from_secs(ttl_hours as u64 * 3600),
        refresh,
        installed_only,
    )
    .await?;
    Ok(boards)
}

// ---------------------------------------------------------------------------------------
// project_create / project_cancel_create
//
// `project_cancel_create` is not in IPC-CONTRACT.md's command table — see SPEC.md §8 open
// question 6. It's the minimum needed to satisfy M2's own acceptance test ("a working
// Cancel"), reusing the real ProcId `project_create` already returns.
// ---------------------------------------------------------------------------------------

#[tauri::command]
pub async fn project_create(
    app: AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    req: CreateProjectRequest,
    on_event: Channel<ProcEvent>,
) -> Result<ProcId, AppError> {
    init::validate_name(&req.name)?;
    let parent = PathBuf::from(&req.parent_dir);
    if !parent.is_dir() {
        return Err(AppError::Io {
            message: format!("{} is not a directory", parent.display()),
        });
    }
    let dir = init::project_dir(&req);
    // `pio project init` itself is fine with a non-empty target directory (that's exactly
    // how "initialize this existing folder" — the NotAPioProject path from `project_open`
    // — works); the only thing worth guarding against here is clobbering a folder that's
    // *already* a PlatformIO project.
    if dir.join("platformio.ini").is_file() {
        return Err(AppError::Io {
            message: format!("{} is already a PlatformIO project", dir.display()),
        });
    }
    // `pio project init -d <dir>` validates `<dir>` as an *existing* directory (Click's
    // `Directory` param type) — it does not create it. Verified live against this machine's
    // PlatformIO Core 6.1.19: `-d <missing-dir>` fails immediately with exit code 2,
    // "Invalid value for '--project-dir': Directory '<dir>' does not exist." Not documented
    // in `CLI-CONTRACT.md` §4.1. The "initialize an existing folder" path (`NotAPioProject`
    // from `project_open`) already has an existing `dir`, so this is a no-op there.
    std::fs::create_dir_all(&dir)?;

    let pio = resolved_pio(&supervisor, &settings_state)
        .await
        .ok_or_else(|| AppError::ToolMissing {
            tool: "pio".into(),
            install_action: true,
        })?;

    let mut args = pio.extra_args.clone();
    args.extend(init::build_init_args(&req, &dir));
    let spec = SpawnSpec {
        program: pio.program.clone(),
        args,
        cwd: parent.clone(),
        env: vec![],
        kind: ProcKind::Pio,
        label: "pio-project-init".into(),
    };

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let proc_id = spawn_with_proc_events(&supervisor, spec, tx).await?;

    // Post-init steps (git init, CLAUDE.md, registry) must complete *before* the frontend
    // is told the create finished — it reacts to `Finished` by calling `project_list`
    // (`useProjects`'s `onCreated` -> `refresh`), and a registry write that's still
    // in-flight at that point means the new project doesn't show up until the next manual
    // refresh (e.g. re-opening the folder). So `Finished` is buffered here and only
    // forwarded once the registry save is done, not relayed the instant it arrives.
    let app_for_task = app.clone();
    let dir_for_task = dir.clone();
    let cache = cache_dir(&app)?;
    let board_id = req.board_id.clone();

    tokio::spawn(async move {
        let mut finished: Option<ProcEvent> = None;
        while let Some(ev) = rx.recv().await {
            if let ProcEvent::Finished { .. } = &ev {
                finished = Some(ev);
                break;
            }
            let _ = on_event.send(ev);
        }

        let Some(finished) = finished else {
            return;
        };
        let last_finished_ok = matches!(&finished, ProcEvent::Finished { success: true, .. });

        if !last_finished_ok {
            let _ = on_event.send(finished);
            return;
        }

        let supervisor = app_for_task.state::<ProcessSupervisor>();

        if req.init_git {
            if let Ok(git) = which::which(if cfg!(windows) { "git.exe" } else { "git" }) {
                let _ = supervisor
                    .spawn_capture(SpawnSpec {
                        program: git,
                        args: vec!["init".into()],
                        cwd: dir_for_task.clone(),
                        env: vec![],
                        kind: ProcKind::Tool,
                        label: "git-init".into(),
                    })
                    .await;
            }
        }

        let mut settings = workspace::new_settings(&req.name, CreatedBy::VibeHardware);
        settings.active_env = Some(req.board_id.clone());
        let _ = workspace::save(&dir_for_task, &settings);

        if req.generate_claude_md {
            if let Some(pio) = resolved_pio(&supervisor, &app_for_task.state::<SettingsState>()).await {
                if let Ok(board) = boards::find_board(&supervisor, &pio, &dir_for_task, &cache, &board_id).await {
                    let ini = std::fs::read_to_string(dir_for_task.join("platformio.ini")).unwrap_or_default();
                    let content = claude_md::regenerate(None, &board, &req.framework, &ini);
                    let _ = std::fs::write(dir_for_task.join("CLAUDE.md"), content);
                }
            }
        }

        let registry_state = app_for_task.state::<ProjectRegistryState>();
        let mut reg = registry_state.0.lock().await;
        registry::upsert(
            &mut reg,
            crate::core::project::types::ProjectRegistryEntry {
                id: settings.id.clone(),
                name: req.name.clone(),
                path: dir_for_task.display().to_string(),
                board_id: Some(req.board_id.clone()),
                active_env: settings.active_env.clone(),
                last_opened: Some(chrono::Utc::now().to_rfc3339()),
                last_build_ok: None,
                trusted: true,
            },
        );
        if let Ok(dir) = crate::commands::settings::config_dir(&app_for_task) {
            let _ = registry::save(&dir, &reg);
        }
        drop(reg);

        let _ = on_event.send(finished);
    });

    Ok(proc_id)
}

#[tauri::command]
pub async fn project_cancel_create(
    supervisor: State<'_, ProcessSupervisor>,
    proc_id: ProcId,
) -> Result<(), AppError> {
    supervisor.terminate(&proc_id).await
}

// ---------------------------------------------------------------------------------------
// project_open / project_list / project_forget
// ---------------------------------------------------------------------------------------

#[tauri::command]
pub async fn project_open(
    app: AppHandle,
    registry_state: State<'_, ProjectRegistryState>,
    path: String,
) -> Result<ProjectEntry, AppError> {
    let dir = workspace::canonicalize_workspace(Path::new(&path)).map_err(|_| AppError::Io {
        message: format!("{path} does not exist"),
    })?;
    if !dir.join("platformio.ini").is_file() {
        return Err(AppError::NotAPioProject {
            path: dir.display().to_string(),
        });
    }

    let mut settings = match workspace::load(&dir)? {
        Some(s) => s,
        None => {
            let name = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "project".into());
            workspace::new_settings(&name, CreatedBy::Opened)
        }
    };

    let ini = std::fs::read_to_string(dir.join("platformio.ini")).unwrap_or_default();
    if settings.active_env.is_none() {
        settings.active_env = project_env::list_env_names(&ini).into_iter().next();
    }
    workspace::save(&dir, &settings)?;

    let board_id = settings
        .active_env
        .as_deref()
        .and_then(|env| project_env::read_value(&ini, &format!("env:{env}"), "board"));

    let mut reg = registry_state.0.lock().await;
    registry::upsert(
        &mut reg,
        crate::core::project::types::ProjectRegistryEntry {
            id: settings.id.clone(),
            name: settings.name.clone(),
            path: dir.display().to_string(),
            board_id,
            active_env: settings.active_env.clone(),
            last_opened: Some(chrono::Utc::now().to_rfc3339()),
            last_build_ok: None,
            trusted: settings.trusted,
        },
    );
    let cfg_dir = crate::commands::settings::config_dir(&app)?;
    registry::save(&cfg_dir, &reg)?;

    Ok(registry::to_ipc_entries(&reg)
        .into_iter()
        .find(|e| e.id == settings.id)
        .expect("just upserted"))
}

#[tauri::command]
pub async fn project_list(registry_state: State<'_, ProjectRegistryState>) -> Result<Vec<ProjectEntry>, AppError> {
    let reg = registry_state.0.lock().await;
    Ok(registry::to_ipc_entries(&reg))
}

#[tauri::command]
pub async fn project_forget(
    app: AppHandle,
    registry_state: State<'_, ProjectRegistryState>,
    id: String,
) -> Result<(), AppError> {
    let mut reg = registry_state.0.lock().await;
    registry::forget(&mut reg, &id);
    let dir = crate::commands::settings::config_dir(&app)?;
    registry::save(&dir, &reg)?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Trust
// ---------------------------------------------------------------------------------------

#[tauri::command]
pub async fn project_scan_trust(path: String) -> Result<TrustScan, AppError> {
    Ok(trust::scan(Path::new(&path)))
}

#[tauri::command]
pub async fn project_trust(
    app: AppHandle,
    registry_state: State<'_, ProjectRegistryState>,
    id: String,
) -> Result<(), AppError> {
    let dir = {
        let reg = registry_state.0.lock().await;
        entry_path(&reg, &id)?
    };
    let mut settings = workspace::load(&dir)?.ok_or_else(|| AppError::Io {
        message: format!("{} has no .vibe/project.json", dir.display()),
    })?;
    settings.trusted = true;
    settings.trust_scan_at = Some(chrono::Utc::now().to_rfc3339());
    workspace::save(&dir, &settings)?;

    let mut reg = registry_state.0.lock().await;
    if let Some(entry) = reg.projects.iter_mut().find(|p| p.id == id) {
        entry.trusted = true;
    }
    let cfg_dir = crate::commands::settings::config_dir(&app)?;
    registry::save(&cfg_dir, &reg)?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Environment switcher
// ---------------------------------------------------------------------------------------

#[tauri::command]
pub async fn project_list_envs(registry_state: State<'_, ProjectRegistryState>, id: String) -> Result<Vec<String>, AppError> {
    let dir = {
        let reg = registry_state.0.lock().await;
        entry_path(&reg, &id)?
    };
    let ini = std::fs::read_to_string(dir.join("platformio.ini"))?;
    Ok(project_env::list_env_names(&ini))
}

#[tauri::command]
pub async fn project_set_env(
    app: AppHandle,
    registry_state: State<'_, ProjectRegistryState>,
    id: String,
    env: String,
) -> Result<(), AppError> {
    let dir = {
        let reg = registry_state.0.lock().await;
        entry_path(&reg, &id)?
    };
    let mut settings = workspace::load(&dir)?.ok_or_else(|| AppError::Io {
        message: format!("{} has no .vibe/project.json", dir.display()),
    })?;
    settings.active_env = Some(env.clone());
    workspace::save(&dir, &settings)?;

    let mut reg = registry_state.0.lock().await;
    if let Some(entry) = reg.projects.iter_mut().find(|p| p.id == id) {
        entry.active_env = Some(env);
    }
    let cfg_dir = crate::commands::settings::config_dir(&app)?;
    registry::save(&cfg_dir, &reg)?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Editor / reveal
// ---------------------------------------------------------------------------------------

#[tauri::command]
pub async fn project_open_in_editor(
    app: AppHandle,
    registry_state: State<'_, ProjectRegistryState>,
    settings_state: State<'_, SettingsState>,
    id: String,
    file: Option<String>,
    line: Option<u32>,
) -> Result<(), AppError> {
    let dir = {
        let reg = registry_state.0.lock().await;
        entry_path(&reg, &id)?
    };
    let editor_settings = settings_state.0.lock().await.editor.clone();

    let resolved = editor::resolve_editor(
        editor_settings.command.as_deref(),
        std::env::var("VISUAL").ok().as_deref(),
        std::env::var("EDITOR").ok().as_deref(),
        std::env::var("PATH").ok().as_deref(),
    );

    match resolved {
        Some(program) => {
            let args = editor::build_open_args(&dir, file.as_deref().map(Path::new), line, &editor_settings.goto_line_arg_template);
            let supervisor = app.state::<ProcessSupervisor>();
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            supervisor
                .spawn_streaming(
                    SpawnSpec {
                        program,
                        args,
                        cwd: dir,
                        env: vec![],
                        kind: ProcKind::Tool,
                        label: "open-in-editor".into(),
                    },
                    tx,
                )
                .await?;
            Ok(())
        }
        None => app
            .opener()
            .open_path(dir.display().to_string(), None::<String>)
            .map_err(|e| AppError::Io { message: e.to_string() }),
    }
}

#[tauri::command]
pub async fn project_reveal(app: AppHandle, registry_state: State<'_, ProjectRegistryState>, id: String) -> Result<(), AppError> {
    let dir = {
        let reg = registry_state.0.lock().await;
        entry_path(&reg, &id)?
    };
    app.opener()
        .reveal_item_in_dir(&dir)
        .map_err(|e| AppError::Io { message: e.to_string() })
}

// ---------------------------------------------------------------------------------------
// CLAUDE.md
// ---------------------------------------------------------------------------------------

#[tauri::command]
pub async fn project_regenerate_claude_md(
    app: AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    id: String,
) -> Result<String, AppError> {
    let (dir, board_id, active_env) = {
        let reg = registry_state.0.lock().await;
        let entry = registry::find(&reg, &id).ok_or_else(|| AppError::Io {
            message: format!("no project registered with id {id}"),
        })?;
        (
            PathBuf::from(&entry.path),
            entry.board_id.clone(),
            entry.active_env.clone(),
        )
    };
    let board_id = board_id.ok_or_else(|| AppError::Io {
        message: "project has no known board id".into(),
    })?;

    let ini = std::fs::read_to_string(dir.join("platformio.ini"))?;
    let framework = active_env
        .as_deref()
        .and_then(|env| project_env::read_value(&ini, &format!("env:{env}"), "framework"))
        .unwrap_or_else(|| "arduino".to_string());

    let pio = resolved_pio(&supervisor, &settings_state)
        .await
        .ok_or_else(|| AppError::ToolMissing {
            tool: "pio".into(),
            install_action: true,
        })?;
    let cache = cache_dir(&app)?;
    let board = boards::find_board(&supervisor, &pio, &dir, &cache, &board_id).await?;

    let claude_md_path = dir.join("CLAUDE.md");
    let existing = std::fs::read_to_string(&claude_md_path).ok();
    let content = claude_md::regenerate(existing.as_deref(), &board, &framework, &ini);
    std::fs::write(&claude_md_path, &content)?;
    Ok(content)
}
