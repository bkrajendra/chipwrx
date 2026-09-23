//! Binary resolution (`TOOLCHAIN-SETUP.md` §3). The macOS `.app` launch environment does
//! not inherit a login shell's `PATH` — this is the classic "works in my terminal, not in
//! the app" bug, so nothing here ever assumes a tool is reachable without resolving it.
//!
//! Order: an explicit setting → the process `PATH` → a per-OS candidate list → (macOS/Linux
//! only) a login-shell probe, run once and cached by the caller.

use crate::core::proc::{ProcKind, ProcessSupervisor, SpawnSpec};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Claude,
    Pio,
    Python,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Linux,
    Windows,
}

pub fn current_platform() -> Platform {
    if cfg!(target_os = "macos") {
        Platform::MacOs
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Linux
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Setting,
    Path,
    Candidate,
    LoginShell,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub program: PathBuf,
    /// Non-empty only for Windows' `py -3` launcher.
    pub extra_args: Vec<String>,
    pub source: Source,
}

/// Command names tried against `PATH`, in order.
fn bare_names(tool: Tool, platform: Platform) -> &'static [&'static str] {
    match (tool, platform) {
        (Tool::Claude, Platform::Windows) => &["claude.exe", "claude"],
        (Tool::Claude, _) => &["claude"],
        (Tool::Pio, Platform::Windows) => &["pio.exe", "pio"],
        (Tool::Pio, _) => &["pio"],
        // `TOOLCHAIN-SETUP.md` §2 lists the probe order as "python3, python, py -3" — not
        // just documentation order. `py` delegates to whatever `PEP 514`/registry entry is
        // registered for that version, which can go stale (a prior install moved or
        // removed) even when a perfectly good `python3`/`python` sits earlier on `PATH`;
        // trying the direct executables first avoids surfacing that launcher's error for a
        // Python install that's actually fine.
        //
        // The explicit `.exe` variants matter beyond style consistency with the
        // Claude/PlatformIO lists above: `which_in` only appends `PATHEXT` extensions to a
        // bare name using the *actual host OS*'s rules, not this function's `platform`
        // parameter — so a bare `"py"` only ever resolves on a real Windows machine. Unit
        // tests exercise the Windows path from Linux CI by construction (`Platform` is a
        // simulated parameter, not `cfg!(windows)`), so without the explicit `.exe` name
        // that coverage silently only ran on whichever OS happened to build the test binary.
        (Tool::Python, Platform::Windows) => &["python3.exe", "python3", "python.exe", "python", "py.exe", "py"],
        (Tool::Python, _) => &["python3", "python"],
    }
}

fn extra_args_for(tool: Tool, program: &Path) -> Vec<String> {
    if tool == Tool::Python
        && program
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case("py"))
            .unwrap_or(false)
    {
        vec!["-3".into()]
    } else {
        Vec::new()
    }
}

/// Literal per-OS candidate paths (`TOOLCHAIN-SETUP.md` §3 table). Pure and takes every
/// external input as a parameter — `home`/`extra_env` are simulated in tests rather than
/// read from the real environment, so this never touches the actual filesystem or env.
///
/// `extra_env` recognized keys: `LOCALAPPDATA` (Windows), `PIO_CORE_DIR`.
pub fn candidate_paths(
    tool: Tool,
    platform: Platform,
    home: &Path,
    extra_env: &HashMap<String, String>,
) -> Vec<PathBuf> {
    match (tool, platform) {
        (Tool::Claude, Platform::Windows) => {
            let mut v = vec![home.join(".local").join("bin").join("claude.exe")];
            if let Some(lad) = extra_env.get("LOCALAPPDATA") {
                v.push(PathBuf::from(lad).join("Programs").join("claude").join("claude.exe"));
            }
            v
        }
        (Tool::Claude, _) => vec![
            home.join(".local").join("bin").join("claude"),
            PathBuf::from("/opt/homebrew/bin/claude"),
            PathBuf::from("/usr/local/bin/claude"),
            PathBuf::from("/usr/bin/claude"),
        ],
        (Tool::Pio, Platform::Windows) => vec![home
            .join(".platformio")
            .join("penv")
            .join("Scripts")
            .join("pio.exe")],
        (Tool::Pio, _) => {
            let mut v = vec![
                home.join(".platformio").join("penv").join("bin").join("pio"),
                PathBuf::from("/opt/homebrew/bin/pio"),
                PathBuf::from("/usr/local/bin/pio"),
            ];
            if let Some(core_dir) = extra_env.get("PIO_CORE_DIR") {
                v.push(PathBuf::from(core_dir).join("penv").join("bin").join("pio"));
            }
            v
        }
        (Tool::Python, Platform::Windows) => {
            let mut v = Vec::new();
            if let Some(lad) = extra_env.get("LOCALAPPDATA") {
                let base = PathBuf::from(lad).join("Programs").join("Python");
                if let Ok(entries) = std::fs::read_dir(&base) {
                    let mut matches: Vec<PathBuf> = entries
                        .filter_map(|e| e.ok())
                        .map(|e| e.path())
                        .filter(|p| {
                            p.file_name()
                                .and_then(|n| n.to_str())
                                .map(|n| n.starts_with("Python3"))
                                .unwrap_or(false)
                        })
                        .map(|p| p.join("python.exe"))
                        .collect();
                    matches.sort();
                    v.append(&mut matches);
                }
            }
            v
        }
        (Tool::Python, _) => vec![
            PathBuf::from("/opt/homebrew/bin/python3"),
            PathBuf::from("/usr/local/bin/python3"),
            PathBuf::from("/usr/bin/python3"),
        ],
    }
}

pub fn home_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    } else {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Steps 1–3: an explicit setting, then `PATH` (via `path_env`, so tests never touch the
/// real process `PATH`), then the per-OS candidate list. Does not spawn any process.
pub fn resolve_sync(
    tool: Tool,
    platform: Platform,
    settings_path: Option<&Path>,
    path_env: Option<&str>,
    home: &Path,
    extra_env: &HashMap<String, String>,
) -> Option<Resolution> {
    if let Some(p) = settings_path {
        if is_executable(p) {
            return Some(Resolution {
                program: p.to_path_buf(),
                extra_args: extra_args_for(tool, p),
                source: Source::Setting,
            });
        }
    }

    for name in bare_names(tool, platform) {
        if let Ok(found) = which::which_in(name, path_env.map(|s| s.to_string()), ".") {
            return Some(Resolution {
                extra_args: extra_args_for(tool, &found),
                program: found,
                source: Source::Path,
            });
        }
    }

    for candidate in candidate_paths(tool, platform, home, extra_env) {
        if is_executable(&candidate) {
            return Some(Resolution {
                extra_args: extra_args_for(tool, &candidate),
                program: candidate,
                source: Source::Candidate,
            });
        }
    }

    None
}

/// Step 4, macOS/Linux only: run the user's login shell non-interactively and take
/// `command -v <tool>`. Callers should do this once and cache the result — never on a hot
/// path — because spawning a login shell can be slow (sourcing `.zshrc`/`.bash_profile`).
pub async fn resolve_via_login_shell(
    tool: Tool,
    shell: &Path,
    supervisor: &ProcessSupervisor,
) -> Option<Resolution> {
    let command_name = bare_names(tool, Platform::MacOs)[0];
    let spec = SpawnSpec {
        program: shell.to_path_buf(),
        args: vec!["-lc".into(), format!("command -v {command_name}")],
        cwd: home_dir().unwrap_or_else(|| PathBuf::from(".")),
        env: vec![],
        kind: ProcKind::Tool,
        label: "login-shell-resolve".into(),
    };
    let output = supervisor.spawn_capture(spec).await.ok()?;
    if output.exit_code != 0 {
        return None;
    }
    let path = PathBuf::from(output.stdout.trim());
    if is_executable(&path) {
        Some(Resolution {
            extra_args: extra_args_for(tool, &path),
            program: path,
            source: Source::LoginShell,
        })
    } else {
        None
    }
}

/// All four steps: settings → `PATH` → candidates → (macOS/Linux) login shell. `shell`
/// should be `$SHELL`, falling back to `/bin/sh` if unset — callers do this once, at app
/// start, and reuse the cached result rather than calling this on a hot path.
pub async fn resolve_full(
    tool: Tool,
    platform: Platform,
    settings_path: Option<&Path>,
    home: &Path,
    extra_env: &HashMap<String, String>,
    shell: Option<&Path>,
    supervisor: &ProcessSupervisor,
) -> Option<Resolution> {
    if let Some(found) = resolve_sync(tool, platform, settings_path, real_path_env().as_deref(), home, extra_env) {
        return Some(found);
    }
    if platform == Platform::Windows {
        return None;
    }
    let shell = shell.map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("/bin/sh"));
    resolve_via_login_shell(tool, &shell, supervisor).await
}

fn real_path_env() -> Option<String> {
    std::env::var("PATH").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_executable(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"#!/bin/sh\necho fake\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(path).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(path, perm).unwrap();
        }
    }

    fn tempdir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vibe-hw-resolve-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn setting_path_wins_over_everything_else() {
        let home = tempdir("setting");
        let set_path = home.join("custom-claude");
        write_executable(&set_path);

        let r = resolve_sync(
            Tool::Claude,
            Platform::Linux,
            Some(&set_path),
            None,
            &home,
            &HashMap::new(),
        )
        .expect("resolved");
        assert_eq!(r.source, Source::Setting);
        assert_eq!(r.program, set_path);
    }

    #[test]
    fn falls_back_when_setting_path_is_not_executable() {
        let home = tempdir("badsetting");
        let missing = home.join("does-not-exist");
        let candidate = home.join(".local").join("bin").join("claude");
        write_executable(&candidate);

        let r = resolve_sync(
            Tool::Claude,
            Platform::Linux,
            Some(&missing),
            None,
            &home,
            &HashMap::new(),
        )
        .expect("resolved");
        assert_eq!(r.source, Source::Candidate);
        assert_eq!(r.program, candidate);
    }

    #[test]
    fn resolves_from_path_env() {
        let dir = tempdir("path");
        let platform = current_platform_for_test();
        let bin_name = bare_names(Tool::Claude, platform)[0];
        let bin = dir.join(bin_name);
        write_executable(&bin);
        let path_env = dir.to_string_lossy().into_owned();

        let r = resolve_sync(
            Tool::Claude,
            platform,
            None,
            Some(&path_env),
            &tempdir("path-home"),
            &HashMap::new(),
        )
        .expect("resolved");
        assert_eq!(r.source, Source::Path);
        assert_eq!(r.program, bin);
    }

    #[test]
    fn resolves_from_candidate_list_when_path_has_nothing() {
        let home = tempdir("candidate");
        let candidate = home.join(".platformio").join("penv").join("bin").join("pio");
        write_executable(&candidate);

        let r = resolve_sync(Tool::Pio, Platform::Linux, None, Some(""), &home, &HashMap::new())
            .expect("resolved");
        assert_eq!(r.source, Source::Candidate);
        assert_eq!(r.program, candidate);
    }

    #[test]
    fn pio_core_dir_env_extends_the_candidate_list() {
        let home = tempdir("piocore");
        let core_dir = tempdir("piocore-target");
        let candidate = core_dir.join("penv").join("bin").join("pio");
        write_executable(&candidate);

        let mut extra = HashMap::new();
        extra.insert("PIO_CORE_DIR".to_string(), core_dir.to_string_lossy().into_owned());

        let r = resolve_sync(Tool::Pio, Platform::Linux, None, Some(""), &home, &extra)
            .expect("resolved");
        assert_eq!(r.program, candidate);
    }

    #[test]
    fn nothing_found_returns_none() {
        let home = tempdir("nothing");
        let r = resolve_sync(Tool::Claude, Platform::Linux, None, Some(""), &home, &HashMap::new());
        assert!(r.is_none());
    }

    #[test]
    fn windows_python_launcher_gets_dash_3_extra_arg() {
        let home = tempdir("py-launcher");
        let py = home.join("py.exe");
        write_executable(&py);
        let path_env = home.to_string_lossy().into_owned();

        let r = resolve_sync(
            Tool::Python,
            Platform::Windows,
            None,
            Some(&path_env),
            &home,
            &HashMap::new(),
        )
        .expect("resolved");
        assert_eq!(r.extra_args, vec!["-3".to_string()]);
    }

    #[test]
    fn windows_python_glob_candidate_matches_versioned_dir() {
        let home = tempdir("py-glob-home");
        let lad = tempdir("py-glob-lad");
        let versioned = lad.join("Programs").join("Python").join("Python312");
        write_executable(&versioned.join("python.exe"));

        let mut extra = HashMap::new();
        extra.insert("LOCALAPPDATA".to_string(), lad.to_string_lossy().into_owned());

        let r = resolve_sync(
            Tool::Python,
            Platform::Windows,
            None,
            Some(""),
            &home,
            &extra,
        )
        .expect("resolved");
        assert_eq!(r.source, Source::Candidate);
        assert!(r.extra_args.is_empty());
        assert!(r.program.ends_with("python.exe"));
    }

    // `which_in` needs a platform-appropriate executable name/extension to match, so PATH
    // tests that don't care about that pick whichever platform this test binary runs on.
    fn current_platform_for_test() -> Platform {
        super::current_platform()
    }
}
