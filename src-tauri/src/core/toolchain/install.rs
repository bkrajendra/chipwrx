//! Install flows for Claude Code and PlatformIO (`TOOLCHAIN-SETUP.md` §4, §6).
//!
//! `CLI-CONTRACT.md` documents the canonical one-liners (`curl ... | bash`, `irm ... |
//! iex`) — that's what's shown to the user as the command preview (FR-SETUP-3). But this
//! app never builds a shell string (`CLAUDE.md` hard rule #2), so each installer actually
//! runs as two plain argv steps with the same net effect: download the script, then
//! execute it directly — never piped into a shell process.

use super::resolve::Platform;
use crate::core::proc::{ProcKind, ProcessSupervisor, SpawnSpec, StdStream};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallTarget {
    Claude,
    Pio,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    /// The canonical one-liner shown to the user before anything runs (FR-SETUP-3) — this
    /// is display text, never executed as a shell string.
    pub command_preview: String,
    pub source_url: String,
}

pub fn install_plan(target: InstallTarget, platform: Platform) -> InstallPlan {
    match (target, platform) {
        (InstallTarget::Claude, Platform::Windows) => InstallPlan {
            command_preview: "irm https://claude.ai/install.ps1 | iex".into(),
            source_url: "https://claude.ai/install.ps1".into(),
        },
        (InstallTarget::Claude, _) => InstallPlan {
            command_preview: "curl -fsSL https://claude.ai/install.sh | bash".into(),
            source_url: "https://claude.ai/install.sh".into(),
        },
        (InstallTarget::Pio, _) => InstallPlan {
            command_preview: "curl -fsSL -o get-platformio.py https://raw.githubusercontent.com/platformio/platformio-core-installer/master/get-platformio.py && python3 get-platformio.py".into(),
            source_url: "https://raw.githubusercontent.com/platformio/platformio-core-installer/master/get-platformio.py".into(),
        },
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "type", content = "data")]
pub enum InstallEvent {
    Started { argv: Vec<String> },
    Line { stream: StdStream, text: String },
    Progress { message: String, fraction: Option<f32> },
    Finished { success: bool, exit_code: i32 },
}

pub type InstallSink = mpsc::UnboundedSender<InstallEvent>;

#[derive(Debug)]
pub struct InstallError(pub String);

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for InstallError {}

/// Runs one step to completion, forwarding its output live as `InstallEvent::Line`s and
/// returning its exit code once it exits.
async fn run_step(
    supervisor: &ProcessSupervisor,
    program: PathBuf,
    args: Vec<String>,
    cwd: &Path,
    label: &str,
    sink: &InstallSink,
) -> Result<i32, InstallError> {
    let mut argv_display = vec![program.display().to_string()];
    argv_display.extend(args.iter().cloned());
    let _ = sink.send(InstallEvent::Started { argv: argv_display });

    let (tx, mut rx) = mpsc::unbounded_channel();
    let spec = SpawnSpec {
        program,
        args,
        cwd: cwd.to_path_buf(),
        env: vec![],
        kind: ProcKind::Installer,
        label: label.to_string(),
    };
    let handle = supervisor
        .spawn_streaming(spec, tx)
        .await
        .map_err(|e| InstallError(e.to_string()))?;

    while let Some(line) = rx.recv().await {
        let _ = sink.send(InstallEvent::Line {
            stream: line.stream,
            text: line.text,
        });
    }

    handle
        .exit_code
        .await
        .map_err(|_| InstallError(format!("{label}: exit code channel dropped")))
}

/// Downloads `url` to `dest` and returns an error (with a `Finished { success: false }`
/// event already sent) if the download itself fails — the caller stops rather than trying
/// to run a script that never arrived.
async fn download(
    supervisor: &ProcessSupervisor,
    curl: &Path,
    url: &str,
    dest: &Path,
    cwd: &Path,
    sink: &InstallSink,
) -> Result<(), InstallError> {
    let exit_code = run_step(
        supervisor,
        curl.to_path_buf(),
        vec!["-fsSL".into(), "-o".into(), dest.display().to_string(), url.into()],
        cwd,
        "download-installer",
        sink,
    )
    .await?;
    if exit_code != 0 {
        let _ = sink.send(InstallEvent::Finished {
            success: false,
            exit_code,
        });
        return Err(InstallError(format!("downloading {url} failed with exit code {exit_code}")));
    }
    Ok(())
}

/// Runs the step that actually installs the tool, then emits the overall `Finished` event.
async fn run_final_step(
    supervisor: &ProcessSupervisor,
    program: PathBuf,
    args: Vec<String>,
    cwd: &Path,
    label: &str,
    sink: &InstallSink,
) -> Result<(), InstallError> {
    let exit_code = run_step(supervisor, program, args, cwd, label, sink).await?;
    let _ = sink.send(InstallEvent::Finished {
        success: exit_code == 0,
        exit_code,
    });
    if exit_code == 0 {
        Ok(())
    } else {
        Err(InstallError(format!("{label} exited with code {exit_code}")))
    }
}

/// Resolved once by the caller — production code uses `resolve_system_tools` below; a test
/// substitutes `FakeCli`'s own path for one or both fields.
#[derive(Debug, Clone)]
pub struct InstallTools {
    pub curl: PathBuf,
    /// `bash` on macOS/Linux, `powershell.exe`/`pwsh.exe` on Windows.
    pub script_runner: PathBuf,
}

/// Resolves `curl` and the platform's script runner from `PATH`, for production callers.
pub fn resolve_system_tools(platform: Platform) -> Result<InstallTools, InstallError> {
    let curl = which::which("curl").map_err(|_| InstallError("curl not found on PATH".into()))?;
    let script_runner = match platform {
        Platform::Windows => which::which("powershell.exe")
            .or_else(|_| which::which("pwsh.exe"))
            .map_err(|_| InstallError("PowerShell not found on PATH".into()))?,
        Platform::MacOs | Platform::Linux => {
            which::which("bash").map_err(|_| InstallError("bash not found on PATH".into()))?
        }
    };
    Ok(InstallTools { curl, script_runner })
}

/// Runs the Claude Code install: download `install.sh`/`install.ps1`, then execute it
/// directly (never piped into a shell — see module docs).
pub async fn run_claude_install(
    supervisor: &ProcessSupervisor,
    platform: Platform,
    tools: &InstallTools,
    workdir: &Path,
    sink: InstallSink,
) -> Result<(), InstallError> {
    std::fs::create_dir_all(workdir).map_err(|e| InstallError(e.to_string()))?;

    match platform {
        Platform::Windows => {
            let script = workdir.join("install.ps1");
            let _ = sink.send(InstallEvent::Progress {
                message: "Downloading installer…".into(),
                fraction: Some(0.1),
            });
            download(supervisor, &tools.curl, "https://claude.ai/install.ps1", &script, workdir, &sink).await?;

            let _ = sink.send(InstallEvent::Progress {
                message: "Running installer…".into(),
                fraction: Some(0.5),
            });
            run_final_step(
                supervisor,
                tools.script_runner.clone(),
                vec![
                    "-NoProfile".into(),
                    "-ExecutionPolicy".into(),
                    "Bypass".into(),
                    "-File".into(),
                    script.display().to_string(),
                ],
                workdir,
                "claude-install",
                &sink,
            )
            .await
        }
        Platform::MacOs | Platform::Linux => {
            let script = workdir.join("install.sh");
            let _ = sink.send(InstallEvent::Progress {
                message: "Downloading installer…".into(),
                fraction: Some(0.1),
            });
            download(supervisor, &tools.curl, "https://claude.ai/install.sh", &script, workdir, &sink).await?;

            let _ = sink.send(InstallEvent::Progress {
                message: "Running installer…".into(),
                fraction: Some(0.5),
            });
            run_final_step(
                supervisor,
                tools.script_runner.clone(),
                vec![script.display().to_string()],
                workdir,
                "claude-install",
                &sink,
            )
            .await
        }
    }
}

/// Runs the PlatformIO Core install: download `get-platformio.py`, then run it with the
/// given (already-resolved, version-checked) Python interpreter.
pub async fn run_pio_install(
    supervisor: &ProcessSupervisor,
    curl: &Path,
    python: PathBuf,
    python_extra_args: Vec<String>,
    workdir: &Path,
    sink: InstallSink,
) -> Result<(), InstallError> {
    std::fs::create_dir_all(workdir).map_err(|e| InstallError(e.to_string()))?;
    let script = workdir.join("get-platformio.py");

    let _ = sink.send(InstallEvent::Progress {
        message: "Downloading installer…".into(),
        fraction: Some(0.1),
    });
    download(
        supervisor,
        curl,
        "https://raw.githubusercontent.com/platformio/platformio-core-installer/master/get-platformio.py",
        &script,
        workdir,
        &sink,
    )
    .await?;

    let _ = sink.send(InstallEvent::Progress {
        message: "Installing PlatformIO Core — this downloads a Python virtualenv and can take a few minutes…".into(),
        fraction: Some(0.4),
    });
    let mut args = python_extra_args;
    args.push(script.display().to_string());
    run_final_step(supervisor, python, args, workdir, "pio-install", &sink).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_plan_uses_curl_bash_on_unix() {
        let plan = install_plan(InstallTarget::Claude, Platform::Linux);
        assert!(plan.command_preview.contains("curl"));
        assert!(plan.command_preview.contains("| bash"));
    }

    #[test]
    fn claude_plan_uses_irm_iex_on_windows() {
        let plan = install_plan(InstallTarget::Claude, Platform::Windows);
        assert!(plan.command_preview.contains("irm"));
        assert!(plan.command_preview.contains("iex"));
    }

    #[test]
    fn pio_plan_matches_cli_contract_two_step_command() {
        let plan = install_plan(InstallTarget::Pio, Platform::MacOs);
        assert!(plan.command_preview.contains("get-platformio.py"));
        assert!(plan.source_url.ends_with("get-platformio.py"));
    }
}
