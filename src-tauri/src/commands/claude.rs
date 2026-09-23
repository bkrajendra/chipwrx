//! Thin Tauri command handlers for the conversational loop. See `IPC-CONTRACT.md` §4.
//! Logic lives in `core::claude::{argv,ndjson,turn,session}`.

use crate::commands::doctor::DoctorState;
use crate::commands::project::{entry_path, ProjectRegistryState};
use crate::commands::settings::SettingsState;
use crate::commands::util::run_blocking;
use crate::core::claude::argv::{build_turn_args, SessionRef};
use crate::core::claude::ids::{SessionId, TurnId};
use crate::core::claude::session::{self, TurnResultMeta, TurnToolCall};
use crate::core::claude::turn::{self, RunTurnRequest};
use crate::core::claude::types::{ChatEvent, TurnRequest};
use crate::core::proc::{ProcId, ProcessSupervisor};
use crate::core::project::{trust, workspace};
use crate::core::redact::redact;
use crate::core::settings::PermissionPolicySetting;
use crate::core::snapshot::engine as snapshot_engine;
use crate::core::toolchain::probes;
use crate::core::toolchain::resolve::{self, Tool};
use crate::core::toolchain::types::ProbeResult;
use crate::core::toolchain::version::{self, ClaudeFeature};
use crate::error::AppError;
use std::collections::HashMap;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex;

/// Maps an in-flight `TurnId` to the `ProcId` `claude_stop_turn` needs to signal it. Entries
/// are removed once the turn's background task observes the process exit.
pub struct ActiveTurnsState(pub Mutex<HashMap<TurnId, ProcId>>);

async fn resolved_claude(supervisor: &ProcessSupervisor, settings: &SettingsState) -> Option<resolve::Resolution> {
    let claude_path = settings.0.lock().await.toolchain.claude_path.clone();
    probes::resolve_tool(Tool::Claude, claude_path.as_deref(), supervisor).await
}

/// `ClaudeFeature::PermissionPromptsNone` feature-gates `--permission-prompts none`
/// (`CLI-CONTRACT.md` §1.4: "v2.1.259+ only; omit on older"). Reads the cached Doctor
/// report rather than probing fresh — commands must not block on a process spawn
/// (`IPC-CONTRACT.md` §10 rule 3) — so an app that hasn't run Doctor yet conservatively
/// omits the flag.
async fn permission_prompts_none_supported(doctor_state: &DoctorState) -> bool {
    let cache = doctor_state.0.lock().await;
    let Some(report) = cache.get() else {
        return false;
    };
    let ProbeResult::Ok { version: v, .. } = &report.claude_binary else {
        return false;
    };
    version::parse_version(v)
        .map(|parsed| version::claude_supports(ClaudeFeature::PermissionPromptsNone, parsed, Some(&report.claude_capabilities)))
        .unwrap_or(false)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri State injection, not genuine parameter sprawl
pub async fn claude_send_turn(
    app: AppHandle,
    supervisor: State<'_, ProcessSupervisor>,
    settings_state: State<'_, SettingsState>,
    registry_state: State<'_, ProjectRegistryState>,
    doctor_state: State<'_, DoctorState>,
    active_turns: State<'_, ActiveTurnsState>,
    req: TurnRequest,
    on_event: Channel<ChatEvent>,
) -> Result<TurnId, AppError> {
    let dir = {
        let reg = registry_state.0.lock().await;
        entry_path(&reg, &req.workspace)?
    };

    let mut settings = workspace::load(&dir)?.ok_or_else(|| AppError::Io {
        message: format!("{} has no .vibe/project.json", dir.display()),
    })?;

    // Hard rule 12 / NFR-S5: `claude -p` runs a workspace's hooks and connects its MCP
    // servers with no prompt of its own. A folder the app didn't create must be trusted
    // (via `project_trust`) before the first turn — enforced here too, not just in the UI.
    if !settings.trusted {
        let scan = trust::scan(&dir);
        return Err(AppError::WorkspaceUntrusted {
            hooks: scan.hooks,
            mcp_servers: scan.mcp_servers,
        });
    }

    let claude = resolved_claude(&supervisor, &settings_state).await.ok_or_else(|| AppError::ToolMissing {
        tool: "claude".into(),
        install_action: true,
    })?;

    let global = settings_state.0.lock().await.clone();
    let policy: PermissionPolicySetting = req
        .policy
        .map(PermissionPolicySetting::from)
        .or(settings.claude.permission_policy)
        .unwrap_or(global.claude.permission_policy);
    let model = req.model.clone().or_else(|| settings.claude.model.clone()).unwrap_or(global.claude.model);

    let session = match settings.claude.session_id.clone() {
        Some(id) => SessionRef::Resume(id),
        None => {
            let new_id = SessionId::new().0;
            settings.claude.session_id = Some(new_id.clone());
            workspace::save(&dir, &settings)?;
            SessionRef::New(new_id)
        }
    };
    let session_id_for_record = session.id().to_string();

    let permission_prompts_none = permission_prompts_none_supported(&doctor_state).await;
    let turn_id = TurnId::new();
    let started_at = chrono::Utc::now().to_rfc3339();

    // `FR-CHAT-9`: attachments are referenced by relative path in the prompt text actually
    // sent to the CLI so Claude's own `Read` tool fetches them — the *displayed*/recorded
    // prompt (`prompt_for_record`, below) stays the user's original clean text.
    let effective_prompt = crate::core::claude::attachments::augment_prompt(&req.prompt, &req.attachments);

    let mut redacted_argv = vec![claude.program.display().to_string()];
    redacted_argv.extend(claude.extra_args.clone());
    redacted_argv.extend(build_turn_args(&effective_prompt, policy, permission_prompts_none, &model, &session));
    let redacted_argv: Vec<String> = redacted_argv.iter().map(|a| redact(a, false)).collect();

    // `FR-SAFE-1`: a snapshot before each turn, so it can always be reverted regardless of
    // what the turn does. Fails the turn outright rather than silently proceeding without
    // a safety net — "This milestone is where the product becomes safe to use on real
    // work" (`ROADMAP.md` M4).
    let snapshot_before = {
        let snap_dir = dir.clone();
        let label = turn_id.0.clone();
        run_blocking(move || snapshot_engine::snapshot(&snap_dir, &label)).await?
    };

    // `NFR-S1`: inject the optional keychain-stored `ANTHROPIC_API_KEY` into this child's
    // environment only — never written to argv, never persisted outside the OS keychain.
    // A keychain read failure (e.g. no keychain access in this environment) degrades to "no
    // key" rather than failing the turn outright; the CLI falls back to subscription auth.
    let api_key_env = run_blocking(crate::core::secrets::get_anthropic_api_key)
        .await
        .unwrap_or(None)
        .map(|key| vec![("ANTHROPIC_API_KEY".to_string(), key)])
        .unwrap_or_default();

    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
    let proc_id = turn::run_turn(
        &supervisor,
        RunTurnRequest {
            claude: &claude,
            workspace: &dir,
            turn_id: turn_id.clone(),
            prompt: &effective_prompt,
            session,
            policy,
            model: &model,
            permission_prompts_none_supported: permission_prompts_none,
            env: api_key_env,
        },
        event_tx,
    )
    .await?;

    active_turns.0.lock().await.insert(turn_id.clone(), proc_id);

    let app_for_task = app.clone();
    let workspace_dir = dir.clone();
    let workspace_id_for_task = req.workspace.clone();
    let turn_id_for_task = turn_id.clone();
    let prompt_for_record = req.prompt.clone();
    let model_for_record = model.clone();

    tokio::spawn(async move {
        let mut collected: Vec<ChatEvent> = Vec::new();
        let mut assistant_text = String::new();
        let mut tool_calls: Vec<TurnToolCall> = Vec::new();
        let mut pending_tool_inputs: HashMap<String, (String, serde_json::Value)> = HashMap::new();
        let mut result_meta: Option<TurnResultMeta> = None;

        while let Some(ev) = event_rx.recv().await {
            match &ev {
                ChatEvent::TextBlock { text, .. } => assistant_text.push_str(text),
                ChatEvent::ToolCallCompleted { tool_use_id, name, input, .. } => {
                    pending_tool_inputs.insert(tool_use_id.clone(), (name.clone(), input.clone()));
                }
                ChatEvent::ToolResult { tool_use_id, is_error, .. } => {
                    if let Some((name, input)) = pending_tool_inputs.remove(tool_use_id) {
                        tool_calls.push(TurnToolCall {
                            tool_use_id: tool_use_id.clone(),
                            name,
                            input,
                            is_error: *is_error,
                        });
                    }
                }
                ChatEvent::Result {
                    subtype,
                    is_error,
                    num_turns,
                    duration_ms,
                    duration_api_ms,
                    total_cost_usd,
                    permission_denials,
                    ..
                } => {
                    result_meta = Some(TurnResultMeta {
                        subtype: subtype.clone(),
                        is_error: *is_error,
                        num_turns: *num_turns,
                        duration_ms: *duration_ms,
                        duration_api_ms: *duration_api_ms,
                        total_cost_usd: *total_cost_usd,
                        permission_denials: permission_denials.clone(),
                    });
                }
                _ => {}
            }
            if ev.is_persistable() {
                collected.push(ev.clone());
            }
            let _ = on_event.send(ev);
        }

        // `IPC-CONTRACT.md` §4: "Emitted after Result, once the snapshot diff has been
        // computed" — also after `Failed` (an interrupted/failed turn can still have
        // written partial edits worth showing).
        let changes = {
            let diff_dir = workspace_dir.clone();
            let snap = snapshot_before.clone();
            run_blocking(move || snapshot_engine::diff_against_working_tree(&diff_dir, &snap)).await
        };
        let changes = match changes {
            Ok(changes) => {
                let _ = on_event.send(ChatEvent::ChangesComputed {
                    turn_id: turn_id_for_task.clone(),
                    snapshot: snapshot_before.clone(),
                    changes: changes.clone(),
                });
                changes
            }
            Err(e) => {
                tracing::warn!("failed to compute post-turn diff: {e}");
                Vec::new()
            }
        };

        // `FR-BUILD-3` Watch policy: "after a turn's changes land, Build runs
        // automatically." A no-op under any other policy, or if nothing changed.
        if !changes.is_empty() {
            crate::commands::pipeline::trigger_watch_build_if_applicable(&app_for_task, &workspace_id_for_task).await;
        }

        let mut record = session::new_turn_record(
            turn_id_for_task.0.clone(),
            session_id_for_record,
            started_at,
            prompt_for_record,
            model_for_record,
            policy,
            redacted_argv,
        );
        record.ended_at = Some(chrono::Utc::now().to_rfc3339());
        record.events = session::persistable_events(&collected);
        record.assistant_text = assistant_text;
        record.tool_calls = tool_calls;
        record.result = result_meta;
        record.snapshot_before = Some(snapshot_before.0.clone());
        record.changes = changes;

        if let Err(e) = session::append_turn(&workspace_dir, &record) {
            tracing::warn!("failed to persist turn record: {e}");
        }
        match session::load_index(&workspace_dir) {
            Ok(mut index) => {
                session::record_turn(&mut index, &record);
                if let Err(e) = session::save_index(&workspace_dir, &index) {
                    tracing::warn!("failed to save session index: {e}");
                }
            }
            Err(e) => tracing::warn!("failed to load session index: {e}"),
        }

        let active = app_for_task.state::<ActiveTurnsState>();
        active.0.lock().await.remove(&turn_id_for_task);
    });

    Ok(turn_id)
}

#[tauri::command]
pub async fn claude_stop_turn(
    supervisor: State<'_, ProcessSupervisor>,
    active_turns: State<'_, ActiveTurnsState>,
    turn_id: TurnId,
    hard: bool,
) -> Result<(), AppError> {
    let proc_id = active_turns.0.lock().await.get(&turn_id).cloned();
    let Some(proc_id) = proc_id else {
        return Ok(()); // already finished — nothing to stop
    };
    if hard {
        supervisor.terminate(&proc_id).await
    } else {
        supervisor.interrupt(&proc_id).await
    }
}

#[tauri::command]
pub async fn claude_new_session(registry_state: State<'_, ProjectRegistryState>, workspace: String) -> Result<SessionId, AppError> {
    let dir = {
        let reg = registry_state.0.lock().await;
        entry_path(&reg, &workspace)?
    };
    let mut settings = workspace::load(&dir)?.ok_or_else(|| AppError::Io {
        message: format!("{} has no .vibe/project.json", dir.display()),
    })?;
    let new_id = SessionId::new();
    settings.claude.session_id = Some(new_id.0.clone());
    workspace::save(&dir, &settings)?;
    Ok(new_id)
}

#[tauri::command]
pub async fn claude_history(
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
    limit: u32,
    before: Option<TurnId>,
) -> Result<Vec<session::TurnRecord>, AppError> {
    let dir = {
        let reg = registry_state.0.lock().await;
        entry_path(&reg, &workspace)?
    };
    let settings = workspace::load(&dir)?.ok_or_else(|| AppError::Io {
        message: format!("{} has no .vibe/project.json", dir.display()),
    })?;
    let Some(session_id) = settings.claude.session_id else {
        return Ok(Vec::new());
    };

    let mut turns = session::load_turns(&dir, &session_id)?;
    if let Some(before_id) = before {
        if let Some(pos) = turns.iter().position(|t| t.turn_id == before_id.0) {
            turns.truncate(pos);
        }
    }
    let limit = limit as usize;
    if turns.len() > limit {
        let start = turns.len() - limit;
        turns.drain(0..start);
    }
    Ok(turns)
}

/// `FR-CHAT-9`: copies a file the user picked (via the frontend's own file-open dialog —
/// this command never shows a dialog itself) into `.vibe/attachments/`, returning the
/// workspace-relative path to add to the next turn's `attachments`.
#[tauri::command]
pub async fn attachment_add(registry_state: State<'_, ProjectRegistryState>, workspace: String, source_path: String) -> Result<String, AppError> {
    let dir = {
        let reg = registry_state.0.lock().await;
        entry_path(&reg, &workspace)?
    };
    crate::core::claude::attachments::add(&dir, std::path::Path::new(&source_path))
}
