//! Integration tests for `core::snapshot::engine` against real `git2` repositories in temp
//! directories — this is testing real local git behavior, not a network/hardware/paid-API
//! dependency, so it doesn't go through `FakeCli` (`ARCHITECTURE.md` §10 scopes that rule
//! to external CLI-backed commands). `ROADMAP.md` M4's acceptance test is the north star
//! here: "a turn that edits three files produces a Changes tab with correct line counts;
//! reverting one file restores it byte-for-byte; reverting the turn restores all three;
//! the user's own `git status` is unchanged throughout."

use std::path::{Path, PathBuf};
use vibe_hardware_lib::core::snapshot::engine;
use vibe_hardware_lib::core::snapshot::types::ChangeStatus;

fn tempdir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vibe-hw-snapshot-test-{label}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(workspace: &Path, rel: &str, content: &str) {
    let path = workspace.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

// -----------------------------------------------------------------------------------------
// Shadow backend — workspace has no `.git` of its own.
// -----------------------------------------------------------------------------------------

#[test]
fn detects_shadow_backend_when_no_git_dir_present() {
    let ws = tempdir("detect-shadow");
    assert_eq!(engine::detect_backend(&ws), engine::Backend::Shadow);
}

#[test]
fn shadow_snapshot_and_diff_reports_added_modified_deleted_with_line_counts() {
    let ws = tempdir("shadow-diff");
    write(&ws, "src/main.cpp", "line1\nline2\nline3\n");
    write(&ws, "platformio.ini", "[env:esp32dev]\nboard = esp32dev\n");

    let snap = engine::snapshot(&ws, "turn-1").expect("snapshot");

    // Three edits: modify an existing file, add a new one, delete an existing one.
    write(&ws, "src/main.cpp", "line1\nCHANGED\nline3\nline4\n");
    write(&ws, "include/config.h", "#define X 1\n");
    std::fs::remove_file(ws.join("platformio.ini")).unwrap();

    let changes = engine::diff_against_working_tree(&ws, &snap).expect("diff");
    assert_eq!(changes.len(), 3, "expected 3 changed files, got {changes:?}");

    let main_cpp = changes.iter().find(|c| c.path == "src/main.cpp").expect("src/main.cpp in diff");
    assert_eq!(main_cpp.status, ChangeStatus::Modified);
    assert_eq!(main_cpp.additions, 2, "one changed + one added line");
    assert_eq!(main_cpp.deletions, 1);
    assert!(!main_cpp.outside_expected_dirs);

    let config_h = changes.iter().find(|c| c.path == "include/config.h").expect("include/config.h in diff");
    assert_eq!(config_h.status, ChangeStatus::Added);
    assert_eq!(config_h.additions, 1);
    assert_eq!(config_h.deletions, 0);
    assert!(!config_h.outside_expected_dirs);

    let ini = changes.iter().find(|c| c.path == "platformio.ini").expect("platformio.ini in diff");
    assert_eq!(ini.status, ChangeStatus::Deleted);
    assert!(!ini.outside_expected_dirs);
}

#[test]
fn shadow_flags_writes_outside_expected_dirs() {
    let ws = tempdir("shadow-outside");
    write(&ws, "src/main.cpp", "content\n");
    let snap = engine::snapshot(&ws, "turn-1").expect("snapshot");

    write(&ws, "scripts/oops.py", "print('not an expected dir')\n");

    let changes = engine::diff_against_working_tree(&ws, &snap).expect("diff");
    let outside = changes.iter().find(|c| c.path == "scripts/oops.py").unwrap();
    assert!(outside.outside_expected_dirs);
}

#[test]
fn shadow_revert_file_restores_byte_for_byte() {
    let ws = tempdir("shadow-revert-file");
    let original = "line1\nline2\nline3\n";
    write(&ws, "src/main.cpp", original);
    let snap = engine::snapshot(&ws, "turn-1").expect("snapshot");

    write(&ws, "src/main.cpp", "totally different content\n");
    engine::revert_file(&ws, &snap, "src/main.cpp").expect("revert file");

    let restored = std::fs::read(ws.join("src/main.cpp")).unwrap();
    assert_eq!(restored, original.as_bytes());
}

#[test]
fn shadow_revert_file_deletes_a_file_the_turn_added() {
    let ws = tempdir("shadow-revert-added");
    write(&ws, "src/main.cpp", "content\n");
    let snap = engine::snapshot(&ws, "turn-1").expect("snapshot");

    write(&ws, "include/new.h", "#pragma once\n");
    assert!(ws.join("include/new.h").exists());

    engine::revert_file(&ws, &snap, "include/new.h").expect("revert file");
    assert!(!ws.join("include/new.h").exists());
}

#[test]
fn shadow_revert_turn_restores_all_three_kinds_of_change() {
    let ws = tempdir("shadow-revert-turn");
    write(&ws, "src/main.cpp", "original main\n");
    write(&ws, "platformio.ini", "original ini\n");
    let snap = engine::snapshot(&ws, "turn-1").expect("snapshot");

    write(&ws, "src/main.cpp", "modified main\n");
    write(&ws, "include/new.h", "added header\n");
    std::fs::remove_file(ws.join("platformio.ini")).unwrap();

    engine::revert_to_snapshot(&ws, &snap).expect("revert turn");

    assert_eq!(std::fs::read_to_string(ws.join("src/main.cpp")).unwrap(), "original main\n");
    assert_eq!(std::fs::read_to_string(ws.join("platformio.ini")).unwrap(), "original ini\n");
    assert!(!ws.join("include/new.h").exists());

    // And a fresh diff against the same snapshot now reports no changes at all.
    let changes = engine::diff_against_working_tree(&ws, &snap).expect("diff after revert");
    assert!(changes.is_empty(), "expected no changes after full revert, got {changes:?}");
}

#[test]
fn shadow_file_diff_returns_before_and_after_text() {
    let ws = tempdir("shadow-file-diff");
    write(&ws, "src/main.cpp", "before\n");
    let snap = engine::snapshot(&ws, "turn-1").expect("snapshot");
    write(&ws, "src/main.cpp", "after\n");

    let diff = engine::file_diff(&ws, &snap, "src/main.cpp").expect("file diff");
    assert_eq!(diff.before.as_deref(), Some("before\n"));
    assert_eq!(diff.after.as_deref(), Some("after\n"));
}

#[test]
fn shadow_leaves_no_vibe_or_pio_dirs_in_the_snapshot() {
    let ws = tempdir("shadow-self-exclude");
    write(&ws, "src/main.cpp", "content\n");
    write(&ws, ".vibe/sessions/s1.jsonl", "{}\n");
    write(&ws, ".pio/build/esp32dev/firmware.bin", "binary-ish");
    let snap = engine::snapshot(&ws, "turn-1").expect("snapshot");

    // Nothing under .vibe or .pio should ever show up in a diff against this snapshot,
    // even after those directories change further — they were never captured.
    write(&ws, ".vibe/sessions/s2.jsonl", "{}\n");
    let changes = engine::diff_against_working_tree(&ws, &snap).expect("diff");
    assert!(changes.iter().all(|c| !c.path.starts_with(".vibe") && !c.path.starts_with(".pio")));
}

// -----------------------------------------------------------------------------------------
// User-repo backend — the workspace *is* a real git repository. The whole point of
// `FR-SAFE-1` is that none of this ever changes what `git status`/`git log` show.
// -----------------------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
struct UserRepoState {
    head_oid: Option<git2::Oid>,
    index_entries: Vec<(String, git2::Oid)>,
    branch: Option<String>,
}

fn capture_user_repo_state(ws: &Path) -> UserRepoState {
    let repo = git2::Repository::open(ws).unwrap();
    let head_oid = repo.head().ok().and_then(|h| h.target());
    let branch = repo.head().ok().and_then(|h| h.shorthand().map(str::to_string));
    let index = repo.index().unwrap();
    let index_entries = index
        .iter()
        .map(|e| (String::from_utf8_lossy(&e.path).into_owned(), e.id))
        .collect();
    UserRepoState {
        head_oid,
        index_entries,
        branch,
    }
}

/// Sets up a real git repo with one committed file and one *staged* (uncommitted) change —
/// exactly the kind of local state a snapshot/revert cycle must never disturb.
fn init_real_repo_with_mixed_state(ws: &Path) {
    write(ws, "src/main.cpp", "committed content\n");
    write(ws, "platformio.ini", "[env:esp32dev]\nboard = esp32dev\n");

    let repo = git2::Repository::init(ws).unwrap();
    let sig = git2::Signature::now("Fay", "fay@example.com").unwrap();
    {
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("src/main.cpp")).unwrap();
        index.add_path(Path::new("platformio.ini")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[]).unwrap();
    }

    // Now stage an uncommitted change the user made themselves, which a snapshot/revert
    // cycle must leave exactly as-is.
    write(ws, "src/main.cpp", "committed content\nuser's own uncommitted edit\n");
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("src/main.cpp")).unwrap();
    index.write().unwrap();
}

#[test]
fn detects_user_repo_backend_when_git_dir_present() {
    let ws = tempdir("detect-user-repo");
    init_real_repo_with_mixed_state(&ws);
    assert_eq!(engine::detect_backend(&ws), engine::Backend::UserRepo);
}

#[test]
fn user_repo_snapshot_diff_revert_cycle_never_changes_git_status() {
    let ws = tempdir("user-repo-status-unchanged");
    init_real_repo_with_mixed_state(&ws);
    let before = capture_user_repo_state(&ws);

    let snap = engine::snapshot(&ws, "turn-1").expect("snapshot");
    assert_eq!(before, capture_user_repo_state(&ws), "snapshot must not change HEAD/index/branch");

    // Simulate a turn's edits: modify, add, delete.
    write(&ws, "src/main.cpp", "committed content\nuser's own uncommitted edit\nclaude's edit\n");
    write(&ws, "include/new.h", "#pragma once\n");
    std::fs::remove_file(ws.join("platformio.ini")).unwrap();
    assert_eq!(before, capture_user_repo_state(&ws), "editing files on disk must not touch the index");

    let changes = engine::diff_against_working_tree(&ws, &snap).expect("diff");
    assert_eq!(changes.len(), 3);
    assert_eq!(before, capture_user_repo_state(&ws), "diffing must not change HEAD/index/branch");

    engine::revert_file(&ws, &snap, "include/new.h").expect("revert one file");
    assert!(!ws.join("include/new.h").exists());
    assert_eq!(before, capture_user_repo_state(&ws), "reverting a file must not change HEAD/index/branch");

    engine::revert_to_snapshot(&ws, &snap).expect("revert whole turn");
    assert_eq!(before, capture_user_repo_state(&ws), "reverting the turn must not change HEAD/index/branch");

    // And the content itself really is back, including the user's own uncommitted edit —
    // proof this operated on the working tree, not by discarding the real index's state.
    assert_eq!(
        std::fs::read_to_string(ws.join("src/main.cpp")).unwrap(),
        "committed content\nuser's own uncommitted edit\n"
    );
    assert_eq!(std::fs::read_to_string(ws.join("platformio.ini")).unwrap(), "[env:esp32dev]\nboard = esp32dev\n");
}

#[test]
fn user_repo_adds_vibe_to_info_exclude_but_never_touches_gitignore() {
    let ws = tempdir("user-repo-exclude");
    init_real_repo_with_mixed_state(&ws);
    assert!(!ws.join(".gitignore").exists());

    engine::snapshot(&ws, "turn-1").expect("snapshot");

    let exclude = std::fs::read_to_string(ws.join(".git").join("info").join("exclude")).unwrap();
    assert!(exclude.lines().any(|l| l.trim() == ".vibe/"));
    assert!(!ws.join(".gitignore").exists(), "must never create/write the user's .gitignore");

    // Idempotent: running it again doesn't duplicate the line.
    engine::snapshot(&ws, "turn-2").expect("snapshot again");
    let exclude2 = std::fs::read_to_string(ws.join(".git").join("info").join("exclude")).unwrap();
    assert_eq!(exclude2.lines().filter(|l| l.trim() == ".vibe/").count(), 1);
}

// -----------------------------------------------------------------------------------------
// Path safety
// -----------------------------------------------------------------------------------------

#[test]
fn revert_file_rejects_path_traversal() {
    let ws = tempdir("traversal");
    write(&ws, "src/main.cpp", "content\n");
    let snap = engine::snapshot(&ws, "turn-1").expect("snapshot");

    let err = engine::revert_file(&ws, &snap, "../../etc/passwd").unwrap_err();
    assert!(matches!(err, vibe_hardware_lib::error::AppError::Io { .. }));
}

#[test]
fn revert_file_rejects_absolute_paths() {
    let ws = tempdir("absolute");
    write(&ws, "src/main.cpp", "content\n");
    let snap = engine::snapshot(&ws, "turn-1").expect("snapshot");

    let target = if cfg!(windows) { "C:\\Windows\\win.ini" } else { "/etc/passwd" };
    let err = engine::revert_file(&ws, &snap, target).unwrap_err();
    assert!(matches!(err, vibe_hardware_lib::error::AppError::Io { .. }));
}

// -----------------------------------------------------------------------------------------
// Reset to last good build (FR-SAFE-6)
// -----------------------------------------------------------------------------------------

#[test]
fn reset_to_last_good_build_is_a_clear_typed_error_before_any_build_ran() {
    let ws = tempdir("reset-no-history");
    write(&ws, "src/main.cpp", "content\n");

    let err = engine::reset_to_last_good_build(&ws).unwrap_err();
    match err {
        vibe_hardware_lib::error::AppError::Io { message } => {
            assert!(message.contains("no build history") || message.contains("no successful build"));
        }
        other => panic!("expected AppError::Io, got {other:?}"),
    }
}

#[test]
fn reset_to_last_good_build_reverts_to_the_recorded_snapshot() {
    let ws = tempdir("reset-with-history");
    write(&ws, "src/main.cpp", "good build content\n");
    let good_snap = engine::snapshot(&ws, "good-build").expect("snapshot");

    write(&ws, "src/main.cpp", "broken edit\n");
    write(
        &ws,
        ".vibe/builds.json",
        &format!(r#"{{"schemaVersion":1,"lastGoodSnapshot":"{}","builds":[]}}"#, good_snap.0),
    );

    let restored = engine::reset_to_last_good_build(&ws).expect("reset");
    assert_eq!(restored, good_snap);
    assert_eq!(std::fs::read_to_string(ws.join("src/main.cpp")).unwrap(), "good build content\n");
}
