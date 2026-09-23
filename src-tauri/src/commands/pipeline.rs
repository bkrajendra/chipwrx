//! Thin Tauri command handlers for the build/upload pipeline. See `IPC-CONTRACT.md` §5.
//! Logic lives in `core::pio::{run,pipeline,build_history,targets}` and `core::diag`.

use crate::commands::monitor::{self, MonitorState};
use crate::commands::project::ProjectRegistryState;
use crate::commands::settings::SettingsState;
use crate::commands::util::{cache_dir, run_blocking};
use crate::core::device::broker::{LeaseHolder, PortBroker};
use crate::core::device::monitor::MonitorEvent;
use crate::core::pio::build_history::{self, BuildKind, BuildRecord, BuildSize, DefectCount};
use crate::core::pio::check as pio_check;
use crate::core::pio::pipeline::{self, PipelineRegistry, PipelineState, PipelineStep};
use crate::core::pio::run as pio_run;
use crate::core::pio::targets::{self, TargetInfo, UNIVERSAL_TARGETS};
use crate::core::pio::test as pio_test;
use crate::core::project::registry;
use crate::core::project::workspace;
use crate::core::proc::events::{spawn_with_proc_events, ProcEvent, Severity, SizeUsage};
use crate::core::proc::{ProcId, ProcKind, ProcessSupervisor, SpawnSpec, StdStream};
use crate::core::settings::PipelinePolicySetting;
use crate::core::snapshot::engine as snapshot_engine;
use crate::core::toolchain::probes;
use crate::core::toolchain::resolve::{self, Tool};
use crate::error::AppError;
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

pub struct PipelineRegistryState(pub Mutex<PipelineRegistry>);
/// Maps a workspace to whatever pipeline `ProcId` is currently running in it, for
/// `pipeline_stop` — mirrors `commands::claude::ActiveTurnsState`'s shape.
pub struct ActivePipelineProcs(pub Mutex<HashMap<String, ProcId>>);

pub(crate) async fn resolved_pio(supervisor: &ProcessSupervisor, settings: &SettingsState) -> Option<resolve::Resolution> {
    let pio_path = settings.0.lock().await.toolchain.pio_path.clone();
    probes::resolve_tool(Tool::Pio, pio_path.as_deref(), supervisor).await
}

pub(crate) async fn workspace_dir_and_env(
    registry_state: &State<'_, ProjectRegistryState>,
    id: &str,
) -> Result<(PathBuf, String), AppError> {
    let reg = registry_state.0.lock().await;
    let entry = registry::find(&reg, id).ok_or_else(|| AppError::Io {
        message: format!("no project registered with id {id}"),
    })?;
    let env = entry.active_env.clone().ok_or_else(|| AppError::Io {
        message: "no active environment set for this project".into(),
    })?;
    Ok((PathBuf::from(&entry.path), env))
}

async fn effective_pipeline_policy(settings_state: &State<'_, SettingsState>, dir: &std::path::Path) -> PipelinePolicySetting {
    let project_override = workspace::load(dir).ok().flatten().and_then(|s| s.pipeline.policy);
    if let Some(p) = project_override {
        return p;
    }
    settings_state.0.lock().await.pipeline.policy
}

fn emit_pipeline_state(app: &AppHandle, workspace: &str, state: &PipelineState) {
    #[derive(serde::Serialize, Clone)]
    struct Payload<'a> {
        workspace: &'a str,
        state: &'a PipelineState,
        since: &'a str,
    }
    let _ = app.emit(
        "pipeline://state",
        Payload {
            workspace,
            state,
            since: &state.since,
        },
    );
}

/// Shared by `pipeline_build`/`pipeline_upload`: spawns, forwards events, and on
/// `Finished` — snapshots on success (`FR-SAFE-6`'s `lastGoodSnapshot`), records a
/// `BuildRecord` (with a size delta re-emitted as a corrected final `ProcEvent::Size`),
/// and transitions the pipeline state.
#[allow(clippy::too_many_arguments)]
async fn run_build_or_upload(
    app: AppHandle,
    supervisor: &ProcessSupervisor,
    dir: PathBuf,
    env: String,
    workspace_id: String,
    kind: BuildKind,
    args: Vec<String>,
    label: &str,
    pio: &resolve::Resolution,
    on_event: Channel<ProcEvent>,
    // `FR-DEV-4`: set only for an upload that preempted a running monitor on this
    // workspace's selected port — `(port, the monitor's own event channel)`. Reattached
    // once the run finishes, success or not.
    reattach_monitor: Option<(String, Channel<MonitorEvent>)>,
) -> Result<ProcId, AppError> {
    // `State<'_, T>` can't be moved into `tokio::spawn`'s `'static` future (its lifetime is
    // tied to this call) — every access re-resolves through `AppHandle::state::<T>()`
    // instead, the same pattern `commands::project::project_create` established in M2.
    {
        let pipeline_state = app.state::<PipelineRegistryState>();
        let mut reg = pipeline_state.0.lock().await;
        let state = reg.transition(&workspace_id, PipelineStep::Building, Some(env.clone()), None);
        emit_pipeline_state(&app, &workspace_id, &state);
    }

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let proc_id = pipeline::spawn_pio_run(supervisor, pio, &dir, args, label, tx).await?;
    {
        let active_procs = app.state::<ActivePipelineProcs>();
        active_procs.0.lock().await.insert(workspace_id.clone(), proc_id.clone());
    }

    {
        let pipeline_state = app.state::<PipelineRegistryState>();
        let mut reg = pipeline_state.0.lock().await;
        let state = reg.transition(&workspace_id, PipelineStep::Building, Some(env.clone()), Some(proc_id.clone()));
        emit_pipeline_state(&app, &workspace_id, &state);
    }

    let app_for_task = app.clone();
    let dir_for_task = dir.clone();
    let env_for_task = env.clone();
    let ws_for_task = workspace_id.clone();
    let build_id = proc_id.0.clone();
    let proc_id_for_return = proc_id.clone();

    tokio::spawn(async move {
        let mut error_count = 0u32;
        let mut warning_count = 0u32;
        let mut last_size: Option<SizeUsage> = None;
        let mut success = false;
        let mut duration_ms = 0u64;

        while let Some(ev) = rx.recv().await {
            match &ev {
                ProcEvent::Defect { defect, .. } => match defect.severity {
                    Severity::Error => error_count += 1,
                    Severity::Warning => warning_count += 1,
                    Severity::Note => {}
                },
                ProcEvent::Size { usage, .. } => last_size = Some(usage.clone()),
                ProcEvent::Finished {
                    success: s, duration_ms: d, ..
                } => {
                    success = *s;
                    duration_ms = *d;
                }
                _ => {}
            }
            let _ = on_event.send(ev);
        }

        let history = build_history::load(&dir_for_task).unwrap_or_else(|_| build_history::BuildHistory::empty());
        let previous_size = history.previous_size(&env_for_task);

        let size = last_size.map(|usage| {
            let mut usage = usage;
            if let Some(prev) = previous_size {
                usage.ram_delta = Some(usage.ram_used as i64 - prev.ram_used as i64);
                usage.flash_delta = Some(usage.flash_used as i64 - prev.flash_used as i64);
            }
            usage
        });
        if let Some(usage) = &size {
            // A second, delta-corrected Size event — the one streamed live during the run
            // has `*_delta: None` (`core::diag::size::SizeParser` has no access to
            // `builds.json`).
            let _ = on_event.send(ProcEvent::Size {
                proc_id: proc_id.clone(),
                usage: usage.clone(),
            });
        }

        let snapshot = if success {
            match run_blocking({
                let d = dir_for_task.clone();
                let label = format!("build-{build_id}");
                move || snapshot_engine::snapshot(&d, &label)
            })
            .await
            {
                Ok(snap) => Some(snap.0),
                Err(e) => {
                    tracing::warn!("post-build snapshot failed: {e}");
                    None
                }
            }
        } else {
            None
        };

        let mut history = history;
        build_history::record_build(
            &mut history,
            BuildRecord {
                id: build_id,
                at: chrono::Utc::now().to_rfc3339(),
                env: env_for_task.clone(),
                kind,
                success,
                duration_ms,
                snapshot,
                size: size.as_ref().map(BuildSize::from),
                defect_count: DefectCount {
                    error: error_count,
                    warning: warning_count,
                },
            },
        );
        if let Err(e) = build_history::save(&dir_for_task, &history) {
            tracing::warn!("failed to save build history: {e}");
        }

        let pipeline_state = app_for_task.state::<PipelineRegistryState>();
        let mut reg = pipeline_state.0.lock().await;
        let state = if success {
            reg.mark_build_ok(&ws_for_task, &dir_for_task, &env_for_task)
        } else {
            reg.transition(&ws_for_task, PipelineStep::Failed, Some(env_for_task.clone()), None)
        };
        emit_pipeline_state(&app_for_task, &ws_for_task, &state);
        drop(reg);

        if kind == BuildKind::Upload {
            crate::commands::device::poll_after_upload(&app_for_task).await;

            if let Some((port, monitor_events)) = reattach_monitor {
                let broker = app_for_task.state::<PortBroker>();
                if let Ok(lease) = broker.acquire(&port, LeaseHolder::Monitor, false) {
                    if monitor::start_session(&app_for_task, &dir_for_task, &env_for_task, &ws_for_task, &port, lease, &monitor_events)
                        .await
                        .is_ok()
                    {
                        let _ = monitor_events.send(MonitorEvent::Reattached { port });
                    }
                }
            }
        }

        let active_procs = app_for_task.state::<ActivePipelineProcs>();
        active_procs.0.lock().await.remove(&ws_for_task);
    });

    Ok(proc_id_for_return)
}

/// `FR-BUILD-3` Watch policy: "after a turn's changes land, Build runs automatically."
/// Called from `commands::claude::claude_send_turn` once a turn's diff is computed. There
/// is no frontend-owned `Channel<ProcEvent>` for a build nobody explicitly requested — a
/// `Channel` only exists because a frontend `invoke()` created and passed one in — so this
/// runs the same spawn + bookkeeping `run_build_or_upload` does (snapshot on success,
/// `BuildRecord`, pipeline-state transitions broadcast over the global `pipeline://state`
/// event, which the frontend can listen to regardless of who triggered the run) without
/// forwarding a line-by-line transcript anywhere; the *next* explicit `pipeline_build` a
/// user runs streams normally.
pub async fn trigger_watch_build_if_applicable(app: &AppHandle, workspace_id: &str) {
    let registry_state = app.state::<ProjectRegistryState>();
    let settings_state = app.state::<SettingsState>();
    let supervisor = app.state::<ProcessSupervisor>();

    let Ok((dir, env)) = workspace_dir_and_env(&registry_state, workspace_id).await else {
        return;
    };
    if !matches!(effective_pipeline_policy(&settings_state, &dir).await, PipelinePolicySetting::Watch) {
        return;
    }
    let Some(pio) = resolved_pio(&supervisor, &settings_state).await else {
        return;
    };

    {
        let pipeline_state = app.state::<PipelineRegistryState>();
        let mut reg = pipeline_state.0.lock().await;
        let state = reg.transition(workspace_id, PipelineStep::Building, Some(env.clone()), None);
        emit_pipeline_state(app, workspace_id, &state);
    }

    let args = pio_run::build_args(&dir, &env);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let Ok(proc_id) = pipeline::spawn_pio_run(&supervisor, &pio, &dir, args, "pio-run-build-watch", tx).await else {
        return;
    };
    {
        let pipeline_state = app.state::<PipelineRegistryState>();
        let mut reg = pipeline_state.0.lock().await;
        let state = reg.transition(workspace_id, PipelineStep::Building, Some(env.clone()), Some(proc_id.clone()));
        emit_pipeline_state(app, workspace_id, &state);
    }

    let mut error_count = 0u32;
    let mut warning_count = 0u32;
    let mut last_size: Option<SizeUsage> = None;
    let mut success = false;
    let mut duration_ms = 0u64;
    while let Some(ev) = rx.recv().await {
        match ev {
            ProcEvent::Defect { defect, .. } => match defect.severity {
                Severity::Error => error_count += 1,
                Severity::Warning => warning_count += 1,
                Severity::Note => {}
            },
            ProcEvent::Size { usage, .. } => last_size = Some(usage),
            ProcEvent::Finished {
                success: s, duration_ms: d, ..
            } => {
                success = s;
                duration_ms = d;
            }
            _ => {}
        }
    }

    let mut history = build_history::load(&dir).unwrap_or_else(|_| build_history::BuildHistory::empty());
    let previous_size = history.previous_size(&env);
    let size = last_size.map(|mut usage| {
        if let Some(prev) = previous_size {
            usage.ram_delta = Some(usage.ram_used as i64 - prev.ram_used as i64);
            usage.flash_delta = Some(usage.flash_used as i64 - prev.flash_used as i64);
        }
        usage
    });

    let snapshot = if success {
        run_blocking({
            let d = dir.clone();
            let label = format!("build-watch-{}", proc_id.0);
            move || snapshot_engine::snapshot(&d, &label)
        })
        .await
        .ok()
        .map(|s| s.0)
    } else {
        None
    };

    build_history::record_build(
        &mut history,
        BuildRecord {
            id: proc_id.0,
            at: chrono::Utc::now().to_rfc3339(),
            env: env.clone(),
            kind: BuildKind::Build,
            success,
            duration_ms,
            snapshot,
            size: size.as_ref().map(BuildSize::from),
            defect_count: DefectCount {
                error: error_count,
                warning: warning_count,
            },
        },
    );
    if let Err(e) = build_history::save(&dir, &history) {
        tracing::warn!("failed to save build history (watch): {e}");
    }

    let pipeline_state = app.state::<PipelineRegistryState>();
    let mut reg = pipeline_state.0.lock().await;
    let state = if success {
        reg.mark_build_ok(workspace_id, &dir, &env)
    } else {
        reg.transition(workspace_id, PipelineStep::Failed, Some(env), None)
    };
    emit_pipeline_state(app, workspace_id, &state);
}

#[tauri::command]
pub async fn pipeline_build(
    app: AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
    on_event: Channel<ProcEvent>,
) -> Result<ProcId, AppError> {
    let (dir, env) = workspace_dir_and_env(&registry_state, &workspace).await?;
    let pio = resolved_pio(&supervisor, &settings_state).await.ok_or_else(|| AppError::ToolMissing {
        tool: "pio".into(),
        install_action: true,
    })?;
    let args = pio_run::build_args(&dir, &env);
    run_build_or_upload(app, &supervisor, dir, env, workspace, BuildKind::Build, args, "pio-run-build", &pio, on_event, None).await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pipeline_upload(
    app: AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    pipeline_state: State<'_, PipelineRegistryState>,
    monitor_state: State<'_, MonitorState>,
    workspace: String,
    on_event: Channel<ProcEvent>,
) -> Result<ProcId, AppError> {
    let (dir, env) = workspace_dir_and_env(&registry_state, &workspace).await?;
    let policy = effective_pipeline_policy(&settings_state, &dir).await;

    // `FR-BUILD-3`: Fast-path's `pio run -t upload` does the build itself, so there's
    // nothing to gate. Safe and Watch both require a current `BuildOk` first.
    if !matches!(policy, PipelinePolicySetting::FastPath) {
        let mut reg = pipeline_state.0.lock().await;
        let state = reg.effective_state(&workspace, &dir, policy);
        if state.step != PipelineStep::BuildOk {
            return Err(AppError::UploadBlocked {
                reason: "Build the current code successfully before uploading.".into(),
            });
        }
    }

    let pio = resolved_pio(&supervisor, &settings_state).await.ok_or_else(|| AppError::ToolMissing {
        tool: "pio".into(),
        install_action: true,
    })?;
    let preferred_port = workspace::load(&dir).ok().flatten().and_then(|s| s.device.preferred_port);
    let args = pio_run::upload_args(&dir, &env, preferred_port.as_deref());

    // `FR-DEV-4`: preempt this workspace's monitor if it's running and the setting allows
    // it — stopping it now (freeing the port) and reattaching once the upload finishes.
    let auto_reattach = settings_state.0.lock().await.pipeline.auto_reattach_monitor;
    let reattach_monitor = if auto_reattach {
        match (monitor_state.preempt(&workspace, "Upload").await, &preferred_port) {
            (Some(channel), Some(port)) => Some((port.clone(), channel)),
            _ => None,
        }
    } else {
        None
    };

    run_build_or_upload(
        app,
        &supervisor,
        dir,
        env,
        workspace,
        BuildKind::Upload,
        args,
        "pio-run-upload",
        &pio,
        on_event,
        reattach_monitor,
    )
    .await
}

#[tauri::command]
pub async fn pipeline_run_target(
    app: AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
    target: String,
    on_event: Channel<ProcEvent>,
) -> Result<ProcId, AppError> {
    let (dir, env) = workspace_dir_and_env(&registry_state, &workspace).await?;
    let pio = resolved_pio(&supervisor, &settings_state).await.ok_or_else(|| AppError::ToolMissing {
        tool: "pio".into(),
        install_action: true,
    })?;
    let args = pio_run::target_args(&dir, &env, &target);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let proc_id = pipeline::spawn_pio_run(&supervisor, &pio, &dir, args, &format!("pio-run-{target}"), tx).await?;
    {
        let active_procs = app.state::<ActivePipelineProcs>();
        active_procs.0.lock().await.insert(workspace.clone(), proc_id.clone());
    }

    let app_for_task = app.clone();
    let ws_for_task = workspace.clone();
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let _ = on_event.send(ev);
        }
        let active_procs = app_for_task.state::<ActivePipelineProcs>();
        active_procs.0.lock().await.remove(&ws_for_task);
    });

    Ok(proc_id)
}

/// Accumulates a spawned pio process's stdout as it streams by (needed because `pio check`/
/// `pio test --json-output` each print their structured result as a single JSON blob at the
/// very end, not incrementally per line the way build diagnostics are) — forwarding every
/// event unchanged to `on_event` as it arrives, and returning the full stdout once
/// `Finished` is seen.
async fn forward_and_capture_stdout(mut rx: tokio::sync::mpsc::UnboundedReceiver<ProcEvent>, on_event: &Channel<ProcEvent>) -> String {
    let mut stdout_buf = String::new();
    while let Some(ev) = rx.recv().await {
        if let ProcEvent::Lines { lines, .. } = &ev {
            for l in lines {
                if matches!(l.stream, StdStream::Stdout) {
                    stdout_buf.push_str(&l.text);
                    stdout_buf.push('\n');
                }
            }
        }
        let finished = matches!(ev, ProcEvent::Finished { .. });
        let _ = on_event.send(ev);
        if finished {
            break;
        }
    }
    stdout_buf
}

/// `FR-BUILD-9`: static analysis feeding the same Problems list as build diagnostics —
/// `pio check --json-output`'s result is synthesized into `ProcEvent::Defect` events
/// (`source: Check`) once the run finishes, on top of the raw `Started`/`Lines`/`Finished`
/// events every pio-backed command streams.
#[tauri::command]
pub async fn pipeline_check(
    app: AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
    on_event: Channel<ProcEvent>,
) -> Result<ProcId, AppError> {
    let (dir, env) = workspace_dir_and_env(&registry_state, &workspace).await?;
    let pio = resolved_pio(&supervisor, &settings_state).await.ok_or_else(|| AppError::ToolMissing {
        tool: "pio".into(),
        install_action: true,
    })?;
    let mut args = pio.extra_args.clone();
    args.extend(pio_check::check_args(&dir, &env));
    let spec = SpawnSpec {
        program: pio.program.clone(),
        args,
        cwd: dir,
        env: vec![],
        kind: ProcKind::Pio,
        label: "pio-check".into(),
    };
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let proc_id = spawn_with_proc_events(&supervisor, spec, tx).await?;
    {
        let active_procs = app.state::<ActivePipelineProcs>();
        active_procs.0.lock().await.insert(workspace.clone(), proc_id.clone());
    }

    let app_for_task = app.clone();
    let ws_for_task = workspace.clone();
    let event_proc_id = proc_id.clone();
    tokio::spawn(async move {
        let stdout = forward_and_capture_stdout(rx, &on_event).await;
        for defect in pio_check::parse_check_json(&stdout) {
            let _ = on_event.send(ProcEvent::Defect {
                proc_id: event_proc_id.clone(),
                defect,
            });
        }
        let active_procs = app_for_task.state::<ActivePipelineProcs>();
        active_procs.0.lock().await.remove(&ws_for_task);
    });

    Ok(proc_id)
}

/// `FR-BUILD-10`: unit tests, rendered as a pass/fail list — `pio test --json-output`'s
/// result is synthesized into one `ProcEvent::TestResult` per environment once the run
/// finishes. `CLI-CONTRACT.md` §5.3: running tests uploads and runs over the serial port,
/// so this preempts a running monitor exactly as `pipeline_upload` does and reattaches it
/// afterward.
#[tauri::command]
pub async fn pipeline_test(
    app: AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    monitor_state: State<'_, MonitorState>,
    workspace: String,
    on_event: Channel<ProcEvent>,
) -> Result<ProcId, AppError> {
    let (dir, env) = workspace_dir_and_env(&registry_state, &workspace).await?;
    let pio = resolved_pio(&supervisor, &settings_state).await.ok_or_else(|| AppError::ToolMissing {
        tool: "pio".into(),
        install_action: true,
    })?;
    let preferred_port = workspace::load(&dir).ok().flatten().and_then(|s| s.device.preferred_port);

    let auto_reattach = settings_state.0.lock().await.pipeline.auto_reattach_monitor;
    let reattach_monitor = if auto_reattach {
        match (monitor_state.preempt(&workspace, "Test").await, &preferred_port) {
            (Some(channel), Some(port)) => Some((port.clone(), channel)),
            _ => None,
        }
    } else {
        None
    };

    let mut args = pio.extra_args.clone();
    args.extend(pio_test::test_args(&dir, &env, preferred_port.as_deref()));
    let spec = SpawnSpec {
        program: pio.program.clone(),
        args,
        cwd: dir.clone(),
        env: vec![],
        kind: ProcKind::Pio,
        label: "pio-test".into(),
    };
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let proc_id = spawn_with_proc_events(&supervisor, spec, tx).await?;
    {
        let active_procs = app.state::<ActivePipelineProcs>();
        active_procs.0.lock().await.insert(workspace.clone(), proc_id.clone());
    }

    let app_for_task = app.clone();
    let dir_for_task = dir.clone();
    let env_for_task = env.clone();
    let ws_for_task = workspace.clone();
    let event_proc_id = proc_id.clone();
    tokio::spawn(async move {
        let stdout = forward_and_capture_stdout(rx, &on_event).await;
        for suite in pio_test::parse_test_json(&stdout) {
            let _ = on_event.send(ProcEvent::TestResult {
                proc_id: event_proc_id.clone(),
                suite,
            });
        }

        if let Some((port, monitor_events)) = reattach_monitor {
            let broker = app_for_task.state::<PortBroker>();
            if let Ok(lease) = broker.acquire(&port, LeaseHolder::Monitor, false) {
                if monitor::start_session(&app_for_task, &dir_for_task, &env_for_task, &ws_for_task, &port, lease, &monitor_events)
                    .await
                    .is_ok()
                {
                    let _ = monitor_events.send(MonitorEvent::Reattached { port });
                }
            }
        }

        let active_procs = app_for_task.state::<ActivePipelineProcs>();
        active_procs.0.lock().await.remove(&ws_for_task);
    });

    Ok(proc_id)
}

#[tauri::command]
pub async fn pipeline_stop(supervisor: State<'_, ProcessSupervisor>, proc_id: ProcId) -> Result<(), AppError> {
    // FR-BUILD-8: kills the whole process tree, not just `pio` — already guaranteed by
    // ProcessSupervisor's process-group/job-object spawn (ARCHITECTURE.md §3).
    supervisor.terminate(&proc_id).await
}

#[tauri::command]
pub async fn pipeline_targets(
    app: AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
    refresh: bool,
) -> Result<Vec<String>, AppError> {
    let (dir, env) = workspace_dir_and_env(&registry_state, &workspace).await?;
    let cache = cache_dir(&app)?;

    if !refresh {
        if let Some(cached) = targets::load_cached(&cache, &env) {
            return Ok(cached.into_iter().map(|t| t.name).collect());
        }
    }

    let Some(pio) = resolved_pio(&supervisor, &settings_state).await else {
        return Ok(UNIVERSAL_TARGETS.iter().map(|s| s.to_string()).collect());
    };

    let mut args = pio.extra_args.clone();
    args.extend(pio_run::list_targets_args(&dir, &env));
    let output = supervisor
        .spawn_capture(SpawnSpec {
            program: pio.program.clone(),
            args,
            cwd: dir.clone(),
            env: vec![],
            kind: ProcKind::Pio,
            label: "pio-list-targets".into(),
        })
        .await;

    let Ok(output) = output else {
        return Ok(UNIVERSAL_TARGETS.iter().map(|s| s.to_string()).collect());
    };
    let parsed: Vec<TargetInfo> = targets::parse_list_targets(&output.stdout);
    if parsed.is_empty() {
        // Not installed yet, or an unrecognized output shape — `FR-BUILD-7`: "the menu
        // shows the universal subset" until a real list has been cached.
        return Ok(UNIVERSAL_TARGETS.iter().map(|s| s.to_string()).collect());
    }
    let _ = targets::save_cache(&cache, &env, &parsed);
    Ok(parsed.into_iter().map(|t| t.name).collect())
}

#[tauri::command]
pub async fn pipeline_state(
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    pipeline_state: State<'_, PipelineRegistryState>,
    workspace: String,
) -> Result<PipelineState, AppError> {
    let (dir, _env) = workspace_dir_and_env(&registry_state, &workspace).await?;
    let policy = effective_pipeline_policy(&settings_state, &dir).await;
    let mut reg = pipeline_state.0.lock().await;
    Ok(reg.effective_state(&workspace, &dir, policy))
}
