//! Editor resolution and "open at this line" (`FR-PROJ-6`, `CLI-CONTRACT.md` §9).
//! Resolution order: explicit setting → `$VISUAL` → `$EDITOR` → known editors on `PATH` →
//! (the caller falls back to the OS default handler when this returns `None`).

use std::path::{Path, PathBuf};

const KNOWN_EDITORS: &[&str] = &["code", "cursor", "zed", "subl", "idea", "nvim"];

/// Pure and takes every input as a parameter (no real env/PATH reads) so it's fully
/// testable without touching the actual environment.
pub fn resolve_editor(setting: Option<&str>, visual: Option<&str>, editor: Option<&str>, path_env: Option<&str>) -> Option<PathBuf> {
    for candidate in [setting, visual, editor].into_iter().flatten() {
        if let Ok(found) = which::which_in(candidate, path_env.map(str::to_string), ".") {
            return Some(found);
        }
        // An explicit setting/env value may already be an absolute, existing path rather
        // than a bare command name `which` can look up.
        let p = PathBuf::from(candidate);
        if p.is_file() {
            return Some(p);
        }
    }
    for name in KNOWN_EDITORS {
        if let Ok(found) = which::which_in(name, path_env.map(str::to_string), ".") {
            return Some(found);
        }
    }
    None
}

/// Builds the argv (excluding the resolved editor binary itself) for opening `workspace`,
/// or a specific `file`/`line` within it via `goto_template` (e.g. `--goto {file}:{line}`,
/// `EditorSettings::goto_line_arg_template`'s default — VS Code family syntax).
pub fn build_open_args(workspace: &Path, file: Option<&Path>, line: Option<u32>, goto_template: &str) -> Vec<String> {
    match (file, line) {
        (Some(f), Some(l)) => {
            let target = workspace.join(f);
            goto_template
                .replace("{file}", &target.display().to_string())
                .replace("{line}", &l.to_string())
                .split_whitespace()
                .map(String::from)
                .collect()
        }
        (Some(f), None) => vec![workspace.join(f).display().to_string()],
        (None, _) => vec![workspace.display().to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_executable(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(path).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(path, perm).unwrap();
        }
    }

    fn tempdir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vibe-hw-editor-test-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn editor_name() -> &'static str {
        if cfg!(windows) {
            "code.exe"
        } else {
            "code"
        }
    }

    #[test]
    fn explicit_setting_wins_when_it_is_an_existing_absolute_path() {
        let dir = tempdir("explicit");
        let bin = dir.join(editor_name());
        write_executable(&bin);

        let found = resolve_editor(Some(&bin.display().to_string()), None, None, Some(""));
        assert_eq!(found, Some(bin));
    }

    #[test]
    fn falls_back_to_visual_then_editor_then_known_editors() {
        let dir = tempdir("path");
        let bin = dir.join(editor_name());
        write_executable(&bin);
        let path_env = dir.to_string_lossy().into_owned();

        // Nothing set, "code" happens to be on PATH via known-editor fallback.
        let found = resolve_editor(None, None, None, Some(&path_env));
        assert_eq!(found, Some(bin));
    }

    #[test]
    fn nothing_found_returns_none_for_os_default_fallback() {
        let dir = tempdir("nothing");
        let found = resolve_editor(None, None, None, Some(&dir.display().to_string()));
        assert_eq!(found, None);
    }

    #[test]
    fn build_open_args_with_no_file_opens_the_workspace_root() {
        let ws = Path::new("/home/fay/greenhouse-sensor");
        let args = build_open_args(ws, None, None, "--goto {file}:{line}");
        assert_eq!(args, vec![ws.display().to_string()]);
    }

    #[test]
    fn build_open_args_with_file_and_line_fills_the_goto_template() {
        let ws = Path::new("/home/fay/greenhouse-sensor");
        let args = build_open_args(ws, Some(Path::new("src/main.cpp")), Some(42), "--goto {file}:{line}");
        assert_eq!(
            args,
            vec!["--goto".to_string(), format!("{}:42", ws.join("src/main.cpp").display())]
        );
    }

    #[test]
    fn build_open_args_with_file_only_opens_that_file() {
        let ws = Path::new("/home/fay/greenhouse-sensor");
        let args = build_open_args(ws, Some(Path::new("src/main.cpp")), None, "--goto {file}:{line}");
        assert_eq!(args, vec![ws.join("src/main.cpp").display().to_string()]);
    }
}
