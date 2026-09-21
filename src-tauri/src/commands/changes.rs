//! Thin Tauri command handlers for the Changes panel. See `IPC-CONTRACT.md` §8. Logic
//! lives in `core::snapshot::engine`.

use crate::commands::project::{entry_path, ProjectRegistryState};
use crate::commands::util::run_blocking;
use crate::core::claude::ids::TurnId;
use crate::core::claude::session;
use crate::core::project::workspace;
use crate::core::snapshot::engine;
use crate::core::snapshot::types::{FileChange, FileDiff, SnapshotId};
use crate::error::AppError;
use std::path::{Path, PathBuf};
use tauri::State;

async fn workspace_dir(registry_state: &State<'_, ProjectRegistryState>, id: &str) -> Result<PathBuf, AppError> {
    let reg = registry_state.0.lock().await;
    entry_path(&reg, id)
}

/// Looks up the `snapshotBefore` this turn recorded (`DATA-MODEL.md` §6) from its
/// persisted `TurnRecord` — every Changes-panel operation pivots off it.
fn snapshot_before_of(dir: &Path, turn_id: &str) -> Result<SnapshotId, AppError> {
    let settings = workspace::load(dir)?.ok_or_else(|| AppError::Io {
        message: format!("{} has no .vibe/project.json", dir.display()),
    })?;
    let session_id = settings.claude.session_id.ok_or_else(|| AppError::Io {
        message: "no active session".into(),
    })?;
    let turns = session::load_turns(dir, &session_id)?;
    let turn = turns
        .into_iter()
        .find(|t| t.turn_id == turn_id)
        .ok_or_else(|| AppError::Io {
            message: format!("no turn {turn_id} recorded"),
        })?;
    turn.snapshot_before.map(SnapshotId).ok_or_else(|| AppError::Io {
        message: format!("turn {turn_id} has no recorded snapshot"),
    })
}

#[tauri::command]
pub async fn changes_for_turn(
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
    turn_id: TurnId,
) -> Result<Vec<FileChange>, AppError> {
    let dir = workspace_dir(&registry_state, &workspace).await?;
    run_blocking(move || {
        let snap = snapshot_before_of(&dir, &turn_id.0)?;
        engine::diff_against_working_tree(&dir, &snap)
    })
    .await
}

#[tauri::command]
pub async fn changes_diff(
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
    turn_id: TurnId,
    path: String,
) -> Result<FileDiff, AppError> {
    let dir = workspace_dir(&registry_state, &workspace).await?;
    run_blocking(move || {
        let snap = snapshot_before_of(&dir, &turn_id.0)?;
        engine::file_diff(&dir, &snap, &path)
    })
    .await
}

#[tauri::command]
pub async fn changes_revert_file(
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
    turn_id: TurnId,
    path: String,
) -> Result<(), AppError> {
    let dir = workspace_dir(&registry_state, &workspace).await?;
    run_blocking(move || {
        let snap = snapshot_before_of(&dir, &turn_id.0)?;
        engine::revert_file(&dir, &snap, &path)?;
        // `FR-SAFE-3`: "Revert is itself snapshotted so it can be undone."
        engine::snapshot(&dir, &format!("revert-file-{}-{}", turn_id.0, uuid::Uuid::new_v4()))?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn changes_revert_turn(
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
    turn_id: TurnId,
) -> Result<(), AppError> {
    let dir = workspace_dir(&registry_state, &workspace).await?;
    run_blocking(move || {
        let snap = snapshot_before_of(&dir, &turn_id.0)?;
        engine::revert_to_snapshot(&dir, &snap)?;
        engine::snapshot(&dir, &format!("revert-turn-{}-{}", turn_id.0, uuid::Uuid::new_v4()))?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn changes_reset_to_last_good_build(
    registry_state: State<'_, ProjectRegistryState>,
    workspace: String,
) -> Result<SnapshotId, AppError> {
    let dir = workspace_dir(&registry_state, &workspace).await?;
    run_blocking(move || engine::reset_to_last_good_build(&dir)).await
}

#[tauri::command]
pub async fn file_read(registry_state: State<'_, ProjectRegistryState>, workspace: String, path: String) -> Result<String, AppError> {
    let dir = workspace_dir(&registry_state, &workspace).await?;
    run_blocking(move || {
        let target = safe_workspace_path(&dir, &path)?;
        Ok(std::fs::read_to_string(target)?)
    })
    .await
}

/// `ARCHITECTURE.md` §9 rule 3: "all file operations assert the canonical path is a
/// descendant of the workspace root." Unlike `core::snapshot::engine`'s revert/diff paths
/// (which only ever look up paths already found inside a git snapshot tree — never
/// arbitrary filesystem entries), a read-only viewer request is untrusted input with
/// nothing else validating it, so this canonicalizes and checks containment rather than
/// only rejecting `..`/absolute paths.
fn safe_workspace_path(workspace: &Path, rel: &str) -> Result<PathBuf, AppError> {
    if rel.is_empty() || Path::new(rel).is_absolute() || rel.split(['/', '\\']).any(|c| c == "..") {
        return Err(AppError::Io {
            message: format!("refusing to read path outside the workspace: {rel}"),
        });
    }
    let candidate = workspace.join(rel);
    let canonical_ws = workspace::canonicalize_workspace(workspace).map_err(|_| AppError::Io {
        message: format!("{} does not exist", workspace.display()),
    })?;
    let canonical_target = workspace::canonicalize_workspace(&candidate).map_err(|_| AppError::Io {
        message: format!("{rel} does not exist"),
    })?;
    if !canonical_target.starts_with(&canonical_ws) {
        return Err(AppError::Io {
            message: format!("refusing to read path outside the workspace: {rel}"),
        });
    }
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vibe-hw-changes-cmd-test-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn safe_workspace_path_accepts_a_file_inside_the_workspace() {
        let ws = tempdir("inside");
        std::fs::create_dir_all(ws.join("src")).unwrap();
        std::fs::write(ws.join("src/main.cpp"), "content").unwrap();
        assert!(safe_workspace_path(&ws, "src/main.cpp").is_ok());
    }

    #[test]
    fn safe_workspace_path_rejects_traversal() {
        let ws = tempdir("traversal");
        assert!(safe_workspace_path(&ws, "../../etc/passwd").is_err());
    }

    #[test]
    fn safe_workspace_path_rejects_absolute_paths() {
        let ws = tempdir("absolute");
        let target = if cfg!(windows) { "C:\\Windows\\win.ini" } else { "/etc/passwd" };
        assert!(safe_workspace_path(&ws, target).is_err());
    }

    #[test]
    fn safe_workspace_path_rejects_a_nonexistent_file() {
        let ws = tempdir("missing");
        assert!(safe_workspace_path(&ws, "src/does_not_exist.cpp").is_err());
    }
}
