//! Safe-policy staleness detection (`FR-BUILD-3`: "Safe (default): Upload is disabled
//! until a Build for the current file state has succeeded. Any file change invalidates
//! it."). This product has no code editor (`CLAUDE.md`), so file changes come from Claude
//! turns or an external editor — either way, there's no in-process edit event to react to.
//! Rather than a filesystem watcher (a new dependency and a background task to manage),
//! this polls: `max_watched_mtime` is checked on demand (serving `pipeline_state`, and
//! before an Upload) against the mtime recorded when `BuildOk` was last entered.

use std::path::Path;
use std::time::SystemTime;

/// `ARCHITECTURE.md` §4.1's own wording for what invalidates `BuildOk` — narrower than
/// `FR-SAFE-4`'s guard-rail list (which also flags `test/`/`data/`, a different concern:
/// warning about unexpected writes, not build staleness).
const WATCHED_DIRS: &[&str] = &["src", "include", "lib"];
const WATCHED_FILE: &str = "platformio.ini";

/// The latest modification time among every file under the watched dirs/file, or `None`
/// if none of them exist yet. Missing/unreadable entries are skipped rather than failing
/// the whole scan — this is a best-effort staleness signal, not a correctness-critical one.
pub fn max_watched_mtime(workspace: &Path) -> Option<SystemTime> {
    let mut max: Option<SystemTime> = None;
    for dir in WATCHED_DIRS {
        visit(&workspace.join(dir), &mut max);
    }
    if let Ok(meta) = std::fs::metadata(workspace.join(WATCHED_FILE)) {
        if let Ok(mtime) = meta.modified() {
            bump(&mut max, mtime);
        }
    }
    max
}

fn visit(dir: &Path, max: &mut Option<SystemTime>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let Ok(file_type) = entry.file_type() else { continue };
        if file_type.is_dir() {
            visit(&entry.path(), max);
        } else if file_type.is_file() {
            if let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) {
                bump(max, mtime);
            }
        }
    }
}

fn bump(max: &mut Option<SystemTime>, candidate: SystemTime) {
    *max = Some(match *max {
        Some(current) => current.max(candidate),
        None => candidate,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    fn tempdir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("vibe-hw-watch-test-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn none_when_nothing_watched_exists() {
        let ws = tempdir("empty");
        assert_eq!(max_watched_mtime(&ws), None);
    }

    #[test]
    fn picks_up_platformio_ini_and_nested_src_files() {
        let ws = tempdir("basic");
        std::fs::write(ws.join("platformio.ini"), "x").unwrap();
        std::fs::create_dir_all(ws.join("src/nested")).unwrap();
        std::fs::write(ws.join("src/nested/main.cpp"), "x").unwrap();
        assert!(max_watched_mtime(&ws).is_some());
    }

    #[test]
    fn ignores_files_outside_the_watched_set() {
        let ws = tempdir("outside");
        std::fs::write(ws.join("README.md"), "x").unwrap();
        assert_eq!(max_watched_mtime(&ws), None);
    }

    #[test]
    fn detects_a_later_edit_as_a_newer_mtime() {
        let ws = tempdir("later-edit");
        std::fs::create_dir_all(ws.join("src")).unwrap();
        std::fs::write(ws.join("src/main.cpp"), "v1").unwrap();
        let first = max_watched_mtime(&ws).expect("first mtime");

        sleep(Duration::from_millis(20));
        std::fs::write(ws.join("src/main.cpp"), "v2 — claude edited this").unwrap();
        let second = max_watched_mtime(&ws).expect("second mtime");

        assert!(second > first, "expected the edit to bump the max mtime");
    }
}
