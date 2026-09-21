//! The nine Doctor probes (`TOOLCHAIN-SETUP.md` §2). Each is independent, timeout-bounded,
//! and never assumes a tool is on `PATH` — see `core::toolchain::resolve`.

use super::resolve::{Resolution, Tool};
use super::types::{ProbeResult, Remediation, RemediationKind};
use super::version::{self, Version};
use crate::core::proc::{ProcKind, ProcessSupervisor, SpawnSpec};
use crate::error::AppError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

const CLAUDE_BINARY_TIMEOUT: Duration = Duration::from_secs(5);
const CLAUDE_AUTH_TIMEOUT: Duration = Duration::from_secs(30);
const PIO_BINARY_TIMEOUT: Duration = Duration::from_secs(5);
const PIO_CORE_DIR_TIMEOUT: Duration = Duration::from_secs(10);
const PYTHON_TIMEOUT: Duration = Duration::from_secs(5);
const NETWORK_TIMEOUT: Duration = Duration::from_secs(20);
#[cfg(target_os = "linux")]
const SERIAL_PERMISSIONS_TIMEOUT: Duration = Duration::from_secs(3);
const GIT_TIMEOUT: Duration = Duration::from_secs(3);

fn tail(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        format!("…{}", &s[s.len() - max_bytes..])
    }
}

async fn capture_version(
    supervisor: &ProcessSupervisor,
    resolution: &Resolution,
    args: &[&str],
    timeout: Duration,
    kind: ProcKind,
    label: &str,
    cwd: &Path,
) -> Result<(i32, String, String), ProbeResult> {
    let mut full_args = resolution.extra_args.clone();
    full_args.extend(args.iter().map(|s| s.to_string()));
    let spec = SpawnSpec {
        program: resolution.program.clone(),
        args: full_args,
        cwd: cwd.to_path_buf(),
        env: vec![],
        kind,
        label: label.to_string(),
    };
    match tokio::time::timeout(timeout, supervisor.spawn_capture(spec)).await {
        Ok(Ok(out)) => Ok((out.exit_code, out.stdout, out.stderr)),
        // A resolved path can still turn out to be a stale PATH/candidate entry (moved or
        // uninstalled since resolution) — report it the same as "never resolved" rather
        // than a raw, confusing OS error, since from the user's perspective the tool is
        // simply not there.
        Ok(Err(AppError::ToolMissing { .. })) => Err(ProbeResult::Missing {
            install_available: true,
        }),
        Ok(Err(e)) => Err(ProbeResult::Error {
            detail: e.to_string(),
        }),
        Err(_) => Err(ProbeResult::Error {
            detail: format!("{label} timed out after {timeout:?}"),
        }),
    }
}

// ---------------------------------------------------------------------------------------
// claudeBinary
// ---------------------------------------------------------------------------------------

pub async fn probe_claude_binary(
    supervisor: &ProcessSupervisor,
    resolution: Option<&Resolution>,
    cwd: &Path,
) -> ProbeResult {
    let Some(resolution) = resolution else {
        return ProbeResult::Missing {
            install_available: true,
        };
    };

    let (exit_code, stdout, stderr) = match capture_version(
        supervisor,
        resolution,
        &["--version"],
        CLAUDE_BINARY_TIMEOUT,
        ProcKind::Claude,
        "claude-binary-probe",
        cwd,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return e,
    };

    if exit_code != 0 {
        return ProbeResult::Error {
            detail: tail(&format!("{stdout}{stderr}"), 4096),
        };
    }

    let Some(found) = version::parse_version(&stdout) else {
        return ProbeResult::Error {
            detail: format!("couldn't parse a version from: {}", tail(&stdout, 200)),
        };
    };

    if !version::meets_minimum(found, version::CLAUDE_MINIMUM) {
        return ProbeResult::Degraded {
            reason: format!(
                "Claude Code {found} is older than the required {}",
                version::CLAUDE_MINIMUM
            ),
            remediation: Some(Remediation {
                kind: RemediationKind::InstallClaude,
                label: "Update Claude Code".into(),
                command_preview: None,
                url: None,
            }),
        };
    }

    ProbeResult::Ok {
        version: found.to_string(),
        path: Some(resolution.program.display().to_string()),
        detail: None,
    }
}

// ---------------------------------------------------------------------------------------
// claudeAuth (+ claudeCapabilities, derived from the same probe turn)
// ---------------------------------------------------------------------------------------

pub struct AuthProbeOutcome {
    pub result: ProbeResult,
    pub capabilities: Vec<String>,
}

fn auth_remediation() -> Remediation {
    Remediation {
        kind: RemediationKind::AuthenticateClaude,
        label: "Sign in".into(),
        command_preview: None,
        url: None,
    }
}

/// Runs the probe turn in `probe_workdir` — a scratch directory the app itself created and
/// controls, never the user's workspace. `claude -p` runs a folder's hooks and connects its
/// MCP servers with no prompt of its own (`CLAUDE.md` landmine 12); an app-owned empty
/// scratch directory has neither.
pub async fn probe_claude_auth(
    supervisor: &ProcessSupervisor,
    resolution: &Resolution,
    probe_workdir: &Path,
) -> AuthProbeOutcome {
    let (exit_code, stdout, stderr) = match capture_version(
        supervisor,
        resolution,
        &[
            "-p",
            "reply with the single word: ok",
            "--output-format",
            "stream-json",
            "--verbose",
            "--permission-mode",
            "dontAsk",
            "--max-turns",
            "1",
        ],
        CLAUDE_AUTH_TIMEOUT,
        ProcKind::Claude,
        "claude-auth-probe",
        probe_workdir,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            return AuthProbeOutcome {
                result: e,
                capabilities: Vec::new(),
            }
        }
    };

    let mut capabilities = Vec::new();
    let mut api_retry_error: Option<String> = None;
    let mut result_line: Option<serde_json::Value> = None;

    for line in stdout.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("system") if v.get("subtype").and_then(|s| s.as_str()) == Some("init") => {
                if let Some(caps) = v.get("capabilities").and_then(|c| c.as_array()) {
                    capabilities = caps
                        .iter()
                        .filter_map(|c| c.as_str().map(String::from))
                        .collect();
                }
            }
            Some("system") if v.get("subtype").and_then(|s| s.as_str()) == Some("api_retry") => {
                api_retry_error = v
                    .get("error")
                    .and_then(|e| e.as_str())
                    .map(String::from)
                    .or(api_retry_error);
            }
            Some("result") => result_line = Some(v),
            _ => {}
        }
    }

    let result = if let Some(result) = &result_line {
        let is_error = result.get("is_error").and_then(|b| b.as_bool()).unwrap_or(true);
        if !is_error {
            ProbeResult::Ok {
                version: String::new(),
                path: None,
                detail: Some("authenticated".into()),
            }
        } else {
            classify_auth_failure(api_retry_error.as_deref(), result)
        }
    } else if let Some(err) = &api_retry_error {
        classify_auth_failure(Some(err), &serde_json::Value::Null)
    } else {
        ProbeResult::Error {
            detail: tail(&format!("exit {exit_code}: {stdout}{stderr}"), 4096),
        }
    };

    AuthProbeOutcome { result, capabilities }
}

fn classify_auth_failure(api_retry_error: Option<&str>, result: &serde_json::Value) -> ProbeResult {
    match api_retry_error {
        Some("authentication_failed") => ProbeResult::Degraded {
            reason: "Claude Code isn't signed in".into(),
            remediation: Some(auth_remediation()),
        },
        Some("account_on_hold") => ProbeResult::Degraded {
            reason: "This account is on hold".into(),
            remediation: Some(auth_remediation()),
        },
        Some("billing_error") => ProbeResult::Degraded {
            reason: "There's a billing problem with this account".into(),
            remediation: Some(auth_remediation()),
        },
        Some("oauth_org_not_allowed") => ProbeResult::Degraded {
            reason: "This account's organization policy blocks Claude Code".into(),
            remediation: Some(auth_remediation()),
        },
        _ => {
            let text = result
                .get("result")
                .and_then(|r| r.as_str())
                .unwrap_or("the probe turn failed");
            ProbeResult::Error {
                detail: tail(text, 4096),
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// pioBinary
// ---------------------------------------------------------------------------------------

pub async fn probe_pio_binary(
    supervisor: &ProcessSupervisor,
    resolution: Option<&Resolution>,
    cwd: &Path,
) -> ProbeResult {
    let Some(resolution) = resolution else {
        return ProbeResult::Missing {
            install_available: true,
        };
    };

    let (exit_code, stdout, stderr) = match capture_version(
        supervisor,
        resolution,
        &["--version"],
        PIO_BINARY_TIMEOUT,
        ProcKind::Pio,
        "pio-binary-probe",
        cwd,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return e,
    };

    if exit_code != 0 {
        return ProbeResult::Error {
            detail: tail(&format!("{stdout}{stderr}"), 4096),
        };
    }

    let Some(found) = version::parse_version(&stdout) else {
        return ProbeResult::Error {
            detail: format!("couldn't parse a version from: {}", tail(&stdout, 200)),
        };
    };

    if !version::meets_minimum(found, version::PIO_MINIMUM) {
        return ProbeResult::Degraded {
            reason: format!(
                "PlatformIO Core {found} is older than the required {}",
                version::PIO_MINIMUM
            ),
            remediation: Some(Remediation {
                kind: RemediationKind::InstallPio,
                label: "Update PlatformIO".into(),
                command_preview: None,
                url: None,
            }),
        };
    }

    ProbeResult::Ok {
        version: found.to_string(),
        path: Some(resolution.program.display().to_string()),
        detail: None,
    }
}

// ---------------------------------------------------------------------------------------
// pioCoreDir — also the source of `core_dir` and `python_exe` for other probes/features.
// ---------------------------------------------------------------------------------------

pub struct PioSystemInfo {
    pub core_version: Option<Version>,
    pub core_dir: Option<PathBuf>,
    pub python_exe: Option<PathBuf>,
    pub dev_platform_nums: Option<u64>,
}

pub struct PioCoreDirOutcome {
    pub result: ProbeResult,
    pub info: Option<PioSystemInfo>,
}

pub async fn probe_pio_core_dir(
    supervisor: &ProcessSupervisor,
    resolution: Option<&Resolution>,
    cwd: &Path,
) -> PioCoreDirOutcome {
    let Some(resolution) = resolution else {
        return PioCoreDirOutcome {
            result: ProbeResult::Missing {
                install_available: false, // nothing to "install" here — pioBinary owns that
            },
            info: None,
        };
    };

    let (exit_code, stdout, stderr) = match capture_version(
        supervisor,
        resolution,
        &["system", "info", "--json-output"],
        PIO_CORE_DIR_TIMEOUT,
        ProcKind::Pio,
        "pio-core-dir-probe",
        cwd,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return PioCoreDirOutcome { result: e, info: None },
    };

    if exit_code != 0 {
        return PioCoreDirOutcome {
            result: ProbeResult::Error {
                detail: tail(&format!("{stdout}{stderr}"), 4096),
            },
            info: None,
        };
    }

    let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) else {
        return PioCoreDirOutcome {
            result: ProbeResult::Error {
                detail: format!("`pio system info --json-output` wasn't JSON: {}", tail(&stdout, 200)),
            },
            info: None,
        };
    };

    let field = |name: &str| -> Option<String> {
        json.get(name)
            .and_then(|f| f.get("value"))
            .and_then(|v| v.as_str())
            .map(String::from)
    };

    let core_version = field("core_version").as_deref().and_then(version::parse_version);
    let core_dir = field("core_dir").map(PathBuf::from);
    let python_exe = field("python_exe").map(PathBuf::from);
    let dev_platform_nums = json
        .get("dev_platform_nums")
        .and_then(|f| f.get("value"))
        .and_then(|v| v.as_u64());

    let info = PioSystemInfo {
        core_version,
        core_dir: core_dir.clone(),
        python_exe,
        dev_platform_nums,
    };

    let result = match &core_dir {
        None => ProbeResult::Error {
            detail: "`pio system info --json-output` had no core_dir field".into(),
        },
        Some(dir) if !dir.exists() => ProbeResult::Degraded {
            reason: format!("PlatformIO's core directory {} does not exist", dir.display()),
            remediation: None,
        },
        Some(dir) if !dir_is_writable(dir) => ProbeResult::Degraded {
            reason: format!("PlatformIO's core directory {} isn't writable", dir.display()),
            remediation: None,
        },
        Some(dir) => {
            let detail = match dev_platform_nums {
                Some(0) => Some("no platforms installed yet — the first project will download one".to_string()),
                _ => None,
            };
            ProbeResult::Ok {
                version: core_version.map(|v| v.to_string()).unwrap_or_default(),
                path: Some(dir.display().to_string()),
                detail,
            }
        }
    };

    PioCoreDirOutcome { result, info: Some(info) }
}

fn dir_is_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".vhw-write-test-{}", std::process::id()));
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------------------
// python — only needed if PlatformIO must be installed
// ---------------------------------------------------------------------------------------

pub async fn probe_python(
    supervisor: &ProcessSupervisor,
    resolution: Option<&Resolution>,
    cwd: &Path,
) -> ProbeResult {
    let Some(resolution) = resolution else {
        return ProbeResult::Missing {
            install_available: false, // the app names a version and links out; never installs Python
        };
    };

    let (exit_code, stdout, stderr) = match capture_version(
        supervisor,
        resolution,
        &["--version"],
        PYTHON_TIMEOUT,
        ProcKind::Tool,
        "python-probe",
        cwd,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return e,
    };

    if exit_code != 0 {
        return ProbeResult::Error {
            detail: tail(&format!("{stdout}{stderr}"), 4096),
        };
    }

    // Some old Python builds print `--version` to stderr instead of stdout.
    let combined = format!("{stdout}{stderr}");
    let Some(found) = version::parse_version(&combined) else {
        return ProbeResult::Error {
            detail: format!("couldn't parse a version from: {}", tail(&combined, 200)),
        };
    };

    if !version::meets_minimum(found, version::PYTHON_MINIMUM) {
        return ProbeResult::Degraded {
            reason: format!("Python {found} is older than the required {}", version::PYTHON_MINIMUM),
            remediation: None,
        };
    }

    ProbeResult::Ok {
        version: found.to_string(),
        path: Some(resolution.program.display().to_string()),
        detail: None,
    }
}

// ---------------------------------------------------------------------------------------
// networkRegistry
// ---------------------------------------------------------------------------------------

pub const REGISTRY_PROBE_URL: &str = "https://api.registry.platformio.org/v3/search?query=test";

/// `url` is a parameter (rather than always `REGISTRY_PROBE_URL`) so tests can point this
/// at an unreachable local address and get a deterministic `Degraded` without any real
/// internet access — connecting to `127.0.0.1` never leaves the machine.
pub async fn probe_network_registry(client: &reqwest::Client, url: &str) -> ProbeResult {
    let request = client.head(url).timeout(NETWORK_TIMEOUT).send();

    match tokio::time::timeout(NETWORK_TIMEOUT, request).await {
        Ok(Ok(_response)) => {
            // Reachability, not a successful API call — even a 404/405 means the registry
            // host answered. `api.registry.platformio.org` has no unauthenticated `HEAD`
            // contract documented, so any response at all is what "reachable" means here.
            ProbeResult::Ok {
                version: String::new(),
                path: None,
                detail: None,
            }
        }
        Ok(Err(e)) => ProbeResult::Degraded {
            reason: format!("PlatformIO's package registry is unreachable: {e}"),
            remediation: None,
        },
        Err(_) => ProbeResult::Degraded {
            reason: "PlatformIO's package registry is unreachable".into(),
            remediation: None,
        },
    }
}

// ---------------------------------------------------------------------------------------
// serialPermissions — Linux only; always Ok elsewhere (`TOOLCHAIN-SETUP.md` §7).
// ---------------------------------------------------------------------------------------

#[cfg(target_os = "linux")]
const UDEV_RULE_PATHS: [&str; 2] = [
    "/etc/udev/rules.d/99-platformio-udev.rules",
    "/lib/udev/rules.d/99-platformio-udev.rules",
];

fn udev_rule_lines(text: &str) -> std::collections::HashSet<&str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

/// Pure so the outdated/missing logic is unit-testable without touching `/etc`.
pub fn evaluate_udev_rules(installed: Option<&str>, canonical: Option<&str>) -> ProbeResult {
    let Some(installed) = installed else {
        return ProbeResult::Degraded {
            reason: "PlatformIO's udev rules aren't installed".into(),
            remediation: Some(Remediation {
                kind: RemediationKind::InstallUdevRules,
                label: "Show install commands".into(),
                command_preview: Some(vec![
                    "sudo cp <canonical-rules-path> /etc/udev/rules.d/99-platformio-udev.rules".into(),
                    "sudo udevadm control --reload-rules && sudo udevadm trigger".into(),
                    "sudo usermod -a -G dialout \"$USER\"".into(),
                ]),
                url: Some("https://docs.platformio.org/en/latest/core/installation/udev-rules.html".into()),
            }),
        };
    };

    if let Some(canonical) = canonical {
        let canonical_lines = udev_rule_lines(canonical);
        let installed_lines = udev_rule_lines(installed);
        if !canonical_lines.is_subset(&installed_lines) {
            return ProbeResult::Degraded {
                reason: "PlatformIO's udev rules are outdated".into(),
                remediation: Some(Remediation {
                    kind: RemediationKind::InstallUdevRules,
                    label: "Show update commands".into(),
                    command_preview: Some(vec![
                        "sudo cp <canonical-rules-path> /etc/udev/rules.d/99-platformio-udev.rules".into(),
                        "sudo udevadm control --reload-rules && sudo udevadm trigger".into(),
                    ]),
                    url: Some("https://docs.platformio.org/en/latest/core/installation/udev-rules.html".into()),
                }),
            };
        }
    }

    ProbeResult::Ok {
        version: String::new(),
        path: None,
        detail: None,
    }
}

pub async fn probe_serial_permissions(
    supervisor: &ProcessSupervisor,
    python_exe: Option<&Path>,
    skip: bool,
    cwd: &Path,
) -> ProbeResult {
    if skip || !cfg!(target_os = "linux") {
        return ProbeResult::Ok {
            version: String::new(),
            path: None,
            detail: None,
        };
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (supervisor, python_exe, cwd);
        unreachable!("guarded by the cfg!(target_os) check above");
    }
    #[cfg(target_os = "linux")]
    {
        let installed = UDEV_RULE_PATHS
            .iter()
            .find_map(|p| std::fs::read_to_string(p).ok());

        let canonical = match python_exe {
            Some(py) => {
                let spec = SpawnSpec {
                    program: py.to_path_buf(),
                    args: vec![
                        "-c".into(),
                        "from platformio.fs import get_platformio_udev_rules_path as p;print(p())".into(),
                    ],
                    cwd: cwd.to_path_buf(),
                    env: vec![],
                    kind: ProcKind::Tool,
                    label: "udev-canonical-path-probe".into(),
                };
                match tokio::time::timeout(SERIAL_PERMISSIONS_TIMEOUT, supervisor.spawn_capture(spec)).await {
                    Ok(Ok(out)) if out.exit_code == 0 => {
                        std::fs::read_to_string(out.stdout.trim()).ok()
                    }
                    _ => None,
                }
            }
            None => None,
        };

        evaluate_udev_rules(installed.as_deref(), canonical.as_deref())
    }
}

// ---------------------------------------------------------------------------------------
// git
// ---------------------------------------------------------------------------------------

pub async fn probe_git(supervisor: &ProcessSupervisor, cwd: &Path) -> ProbeResult {
    // `git` is looked up on `PATH` only — it has no candidate-path table in
    // `TOOLCHAIN-SETUP.md` §3, unlike claude/pio/python.
    let bare_name = if cfg!(windows) { "git.exe" } else { "git" };
    let Ok(program) = which::which(bare_name) else {
        return ProbeResult::Degraded {
            reason: "git wasn't found — snapshots will use the bundled libgit2 fallback".into(),
            remediation: None,
        };
    };

    let resolution = Resolution {
        program,
        extra_args: Vec::new(),
        source: super::resolve::Source::Path,
    };
    let (exit_code, stdout, stderr) = match capture_version(
        supervisor,
        &resolution,
        &["--version"],
        GIT_TIMEOUT,
        ProcKind::Tool,
        "git-probe",
        cwd,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return e,
    };

    if exit_code != 0 {
        return ProbeResult::Degraded {
            reason: tail(&format!("{stdout}{stderr}"), 512),
            remediation: None,
        };
    }

    let version = version::parse_version(&stdout).map(|v| v.to_string()).unwrap_or_default();
    ProbeResult::Ok {
        version,
        path: Some(resolution.program.display().to_string()),
        detail: None,
    }
}

/// Convenience used by the orchestrator: resolves a tool the same way for every probe that
/// needs it, given already-loaded settings paths.
pub async fn resolve_tool(
    tool: Tool,
    settings_path: Option<&str>,
    supervisor: &ProcessSupervisor,
) -> Option<Resolution> {
    let platform = super::resolve::current_platform();
    let home = super::resolve::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let mut extra_env = HashMap::new();
    if let Ok(lad) = std::env::var("LOCALAPPDATA") {
        extra_env.insert("LOCALAPPDATA".to_string(), lad);
    }
    if let Ok(core_dir) = std::env::var("PIO_CORE_DIR") {
        extra_env.insert("PIO_CORE_DIR".to_string(), core_dir);
    }
    let shell = std::env::var("SHELL").ok().map(PathBuf::from);
    super::resolve::resolve_full(
        tool,
        platform,
        settings_path.map(Path::new),
        &home,
        &extra_env,
        shell.as_deref(),
        supervisor,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn udev_rules_missing() {
        let r = evaluate_udev_rules(None, Some("SUBSYSTEM==\"usb\"\n"));
        assert!(matches!(r, ProbeResult::Degraded { .. }));
    }

    #[test]
    fn udev_rules_up_to_date_when_installed_is_a_superset() {
        let canonical = "RULE_A\nRULE_B\n";
        let installed = "# header comment\nRULE_A\nRULE_B\nRULE_C_EXTRA\n";
        let r = evaluate_udev_rules(Some(installed), Some(canonical));
        assert!(matches!(r, ProbeResult::Ok { .. }));
    }

    #[test]
    fn udev_rules_outdated_when_canonical_has_rules_installed_lacks() {
        let canonical = "RULE_A\nRULE_B\nRULE_NEW\n";
        let installed = "RULE_A\nRULE_B\n";
        let r = evaluate_udev_rules(Some(installed), Some(canonical));
        match r {
            ProbeResult::Degraded { reason, .. } => assert!(reason.contains("outdated")),
            other => panic!("expected Degraded, got {other:?}"),
        }
    }

    #[test]
    fn udev_rules_ok_without_a_canonical_copy_to_compare() {
        let r = evaluate_udev_rules(Some("RULE_A\n"), None);
        assert!(matches!(r, ProbeResult::Ok { .. }));
    }

    #[test]
    fn blank_lines_and_comments_are_ignored_in_the_subset_check() {
        let canonical = "\n# comment\nRULE_A\n\n";
        let installed = "RULE_A\n# a different comment\n\n";
        let r = evaluate_udev_rules(Some(installed), Some(canonical));
        assert!(matches!(r, ProbeResult::Ok { .. }));
    }
}
