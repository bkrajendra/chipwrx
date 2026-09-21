//! The change-safety-net engine (`ARCHITECTURE.md` §3 `core/snapshot`, `FR-SAFE-1..6`).
//!
//! Every snapshot — regardless of backend — is built by walking the workspace's files on
//! disk directly and constructing a git tree object by hand (`build_tree`, via
//! [`git2::TreeBuilder`]), rather than through git2's `Index`/`add_all`. That's a
//! deliberate choice: for a workspace that's the *user's own* git repository, touching
//! their real index (which `Index::add_all` would) changes what their next `git status` or
//! `git commit` sees — exactly what `FR-SAFE-1` forbids ("the user's own VCS is never
//! touched"). Using the same file-walk path for both backends means there's one thing to
//! get right instead of two, and the shadow-repo case never depends on git2's ignore-file
//! semantics working exactly as expected.
//!
//! Every commit this module creates is parentless and reachable only via its own
//! `refs/vibe/snapshots/<label>` ref (`git2::Repository::commit` with `update_ref: None`,
//! then a plumbing `reference()` call) — `HEAD`, the index, and the user's own branches are
//! never touched, in either backend.

use super::types::{ChangeStatus, FileChange, FileDiff, SnapshotId};
use crate::error::{AppError, Result};
use git2::{Delta, Oid, Patch, Repository, RepositoryInitOptions, Signature};
use std::path::{Path, PathBuf};

/// `FR-SAFE-4`'s expected-directory allowlist.
const EXPECTED_DIRS: &[&str] = &["src", "include", "lib", "test", "data"];
/// `FR-SAFE-4`'s single expected file.
const EXPECTED_FILE: &str = "platformio.ini";
/// Top-level entries the snapshot walk never descends into: the app's own state, the
/// user's real git metadata, and PlatformIO's build cache (large, binary, regenerated on
/// every build — not source, and not what `FR-SAFE-2`'s diff is for).
const SKIP_AT_ROOT: &[&str] = &[".git", ".vibe", ".pio"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// The workspace is (the root of) the user's own git repository.
    UserRepo,
    /// `.vibe/shadow/` — a separate git dir this app owns, `--work-tree`-equivalent
    /// pointed at the workspace.
    Shadow,
}

pub fn detect_backend(workspace: &Path) -> Backend {
    if workspace.join(".git").exists() {
        Backend::UserRepo
    } else {
        Backend::Shadow
    }
}

fn open_repo(workspace: &Path) -> Result<(Repository, Backend)> {
    match detect_backend(workspace) {
        Backend::UserRepo => {
            let repo = Repository::open(workspace)?;
            ensure_user_repo_excludes_vibe(&repo)?;
            Ok((repo, Backend::UserRepo))
        }
        Backend::Shadow => Ok((open_or_init_shadow(workspace)?, Backend::Shadow)),
    }
}

fn open_or_init_shadow(workspace: &Path) -> Result<Repository> {
    let git_dir = workspace.join(".vibe").join("shadow");
    let repo = if git_dir.join("HEAD").is_file() {
        Repository::open_bare(&git_dir)?
    } else {
        std::fs::create_dir_all(&git_dir)?;
        let mut opts = RepositoryInitOptions::new();
        opts.bare(true);
        Repository::init_opts(&git_dir, &opts)?
    };
    // `update_gitlink: false` — this only points *this* Repository handle's in-process
    // notion of "workdir" at the project root; it never writes a `.git` file into the
    // workspace or touches the shadow repo's own config. Set on every open since it isn't
    // persisted (see `git2::Repository::set_workdir`'s doc comment).
    repo.set_workdir(workspace, false)?;
    Ok(repo)
}

/// `DATA-MODEL.md` §2 / hard rule 11: never touch the user's own `.gitignore`. This is
/// git's *local, uncommitted* equivalent — appended to, not overwritten, so a rerun is a
/// no-op and any exclude the user already added survives.
fn ensure_user_repo_excludes_vibe(repo: &Repository) -> Result<()> {
    let exclude_path = repo.path().join("info").join("exclude");
    let existing = std::fs::read_to_string(&exclude_path).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == ".vibe/") {
        return Ok(());
    }
    std::fs::create_dir_all(exclude_path.parent().expect("info/exclude always has a parent"))?;
    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(".vibe/\n");
    std::fs::write(&exclude_path, updated)?;
    Ok(())
}

fn signature() -> Result<Signature<'static>> {
    Ok(Signature::now("Vibe Hardware", "vibe-hardware@localhost")?)
}

/// Ref names are restricted to what a `turn_id` (a UUID) or this module's own generated
/// labels ever produce, but sanitizing is cheap insurance against a future caller passing
/// something git would reject as a ref-name component.
fn ref_safe(label: &str) -> String {
    label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_' { c } else { '_' })
        .collect()
}

fn snapshot_ref_name(label: &str) -> String {
    format!("refs/vibe/snapshots/{}", ref_safe(label))
}

/// Recursively builds a git tree object mirroring what's on disk under `dir`, skipping
/// [`SKIP_AT_ROOT`] entries at the workspace root. An empty directory is omitted (matches
/// git's own tree model — directories aren't tracked, only files).
fn build_tree(repo: &Repository, dir: &Path, is_root: bool) -> Result<Oid> {
    let mut builder = repo.treebuilder(None)?;
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let name = entry.file_name();
        if is_root && SKIP_AT_ROOT.iter().any(|s| name == std::ffi::OsStr::new(s)) {
            continue;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else { continue };

        if file_type.is_symlink() {
            if let Ok(target) = std::fs::read_link(&path) {
                let oid = repo.blob(target.to_string_lossy().as_bytes())?;
                builder.insert(name, oid, 0o120000)?;
            }
        } else if file_type.is_dir() {
            let sub_oid = build_tree(repo, &path, false)?;
            if repo.find_tree(sub_oid)?.iter().count() > 0 {
                builder.insert(name, sub_oid, 0o040000)?;
            }
        } else if file_type.is_file() {
            let oid = repo.blob_path(&path)?;
            builder.insert(name, oid, file_mode(&path))?;
        }
    }

    Ok(builder.write()?)
}

#[cfg(unix)]
fn file_mode(path: &Path) -> i32 {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(meta) if meta.permissions().mode() & 0o111 != 0 => 0o100755,
        _ => 0o100644,
    }
}

#[cfg(not(unix))]
fn file_mode(_path: &Path) -> i32 {
    0o100644
}

/// Takes a snapshot of the workspace's current on-disk state. `label` becomes (a
/// sanitized form of) the snapshot's ref name — callers pass a `TurnId` for a pre-turn
/// snapshot, or a locally-generated label for a post-revert one (`FR-SAFE-3`: "revert is
/// itself snapshotted").
pub fn snapshot(workspace: &Path, label: &str) -> Result<SnapshotId> {
    let (repo, _backend) = open_repo(workspace)?;
    let tree_oid = build_tree(&repo, workspace, true)?;
    let tree = repo.find_tree(tree_oid)?;
    let sig = signature()?;
    let message = format!("vibe-hardware snapshot: {label}");

    // `update_ref: None` — creates the commit object without touching HEAD or any branch.
    let commit_oid = repo.commit(None, &sig, &sig, &message, &tree, &[])?;
    repo.reference(&snapshot_ref_name(label), commit_oid, true, &message)?;

    Ok(SnapshotId(commit_oid.to_string()))
}

fn find_snapshot_tree<'r>(repo: &'r Repository, id: &SnapshotId) -> Result<git2::Tree<'r>> {
    let oid = Oid::from_str(&id.0).map_err(|e| AppError::SnapshotFailed {
        message: format!("not a valid snapshot id: {e}"),
    })?;
    Ok(repo.find_commit(oid)?.tree()?)
}

/// `FR-SAFE-4`: a change outside the expected source dirs (and `platformio.ini`) gets a
/// warning banner rather than being treated the same as an ordinary source edit.
fn is_outside_expected(path: &str) -> bool {
    if path == EXPECTED_FILE {
        return false;
    }
    let first = path.split('/').next().unwrap_or("");
    !EXPECTED_DIRS.contains(&first)
}

fn diff_trees(repo: &Repository, old: Option<&git2::Tree>, new: Option<&git2::Tree>) -> Result<Vec<FileChange>> {
    let diff = repo.diff_tree_to_tree(old, new, None)?;
    let mut out = Vec::new();

    for (idx, delta) in diff.deltas().enumerate() {
        let status = match delta.status() {
            Delta::Added => ChangeStatus::Added,
            Delta::Deleted => ChangeStatus::Deleted,
            Delta::Renamed => ChangeStatus::Renamed,
            _ => ChangeStatus::Modified,
        };
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let (additions, deletions) = match Patch::from_diff(&diff, idx) {
            Ok(Some(patch)) => patch.line_stats().map(|(_, a, d)| (a as u32, d as u32)).unwrap_or((0, 0)),
            _ => (0, 0),
        };

        out.push(FileChange {
            outside_expected_dirs: is_outside_expected(&path),
            path,
            status,
            additions,
            deletions,
        });
    }
    Ok(out)
}

/// `FR-SAFE-2`: the diff a turn's Changes panel shows — between the snapshot taken before
/// it and the *current* working tree (see `SPEC.md` §8 open question 14 for why this isn't
/// snapshot-vs-snapshot).
pub fn diff_against_working_tree(workspace: &Path, snapshot_id: &SnapshotId) -> Result<Vec<FileChange>> {
    let (repo, _backend) = open_repo(workspace)?;
    let old_tree = find_snapshot_tree(&repo, snapshot_id)?;
    let new_tree_oid = build_tree(&repo, workspace, true)?;
    let new_tree = repo.find_tree(new_tree_oid)?;
    diff_trees(&repo, Some(&old_tree), Some(&new_tree))
}

fn read_blob_text(repo: &Repository, tree: &git2::Tree, path: &str) -> Option<String> {
    let entry = tree.get_path(Path::new(path)).ok()?;
    let obj = entry.to_object(repo).ok()?;
    String::from_utf8(obj.as_blob()?.content().to_vec()).ok()
}

/// The read-only per-file diff (`FR-SAFE-2`): `before` from the snapshot, `after` from
/// disk right now. Either side is `None` for an added/deleted file, or a binary/non-UTF-8
/// one — this app has no code editor, so a diff it can't render as text just shows nothing
/// on that side rather than mangled bytes.
pub fn file_diff(workspace: &Path, snapshot_id: &SnapshotId, path: &str) -> Result<FileDiff> {
    let (repo, _backend) = open_repo(workspace)?;
    let old_tree = find_snapshot_tree(&repo, snapshot_id)?;
    let before = read_blob_text(&repo, &old_tree, path);
    let after = std::fs::read_to_string(safe_join(workspace, path)?).ok();
    Ok(FileDiff {
        path: path.to_string(),
        before,
        after,
    })
}

/// `NFR-S4`: any path reaching a filesystem operation is validated first. Rejects
/// absolute paths and any `..` component so a crafted `path` can never write/delete
/// outside the workspace.
fn safe_join(workspace: &Path, rel: &str) -> Result<PathBuf> {
    if rel.is_empty() || Path::new(rel).is_absolute() || rel.split(['/', '\\']).any(|c| c == "..") {
        return Err(AppError::Io {
            message: format!("refusing to touch path outside the workspace: {rel}"),
        });
    }
    Ok(workspace.join(rel))
}

/// Restores `path` to its content in `snapshot_id`, or deletes it if the snapshot has no
/// such file (the turn added it). A plain filesystem write/delete — never a `git
/// checkout`, so it never touches the index (`FR-SAFE-3`).
pub fn revert_file(workspace: &Path, snapshot_id: &SnapshotId, path: &str) -> Result<()> {
    let (repo, _backend) = open_repo(workspace)?;
    let old_tree = find_snapshot_tree(&repo, snapshot_id)?;
    let target = safe_join(workspace, path)?;

    match old_tree.get_path(Path::new(path)) {
        Ok(entry) => {
            let obj = entry.to_object(&repo)?;
            let blob = obj.as_blob().ok_or_else(|| AppError::SnapshotFailed {
                message: format!("{path} is not a file in this snapshot"),
            })?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, blob.content())?;
        }
        Err(_) => {
            if target.is_file() {
                std::fs::remove_file(&target)?;
            }
        }
    }
    Ok(())
}

/// Reverts every file that differs between `snapshot_id` and the current working tree.
pub fn revert_to_snapshot(workspace: &Path, snapshot_id: &SnapshotId) -> Result<()> {
    for change in diff_against_working_tree(workspace, snapshot_id)? {
        revert_file(workspace, snapshot_id, &change.path)?;
    }
    Ok(())
}

/// `FR-SAFE-6`: reads `.vibe/builds.json`'s `lastGoodSnapshot` (`DATA-MODEL.md` §7 —
/// written by the build pipeline, M5) and reverts to it. `SPEC.md` §8 open question 13:
/// functional but inert until M5 starts writing that file.
pub fn reset_to_last_good_build(workspace: &Path) -> Result<SnapshotId> {
    let builds_path = workspace.join(".vibe").join("builds.json");
    let bytes = std::fs::read(&builds_path).map_err(|_| AppError::Io {
        message: "no build history recorded yet — build the project at least once".into(),
    })?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    let snapshot_str = value
        .get("lastGoodSnapshot")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Io {
            message: "no successful build recorded yet".into(),
        })?;
    let snapshot_id = SnapshotId(snapshot_str.to_string());
    revert_to_snapshot(workspace, &snapshot_id)?;
    Ok(snapshot_id)
}
