//! `ProcessSupervisor` — the single choke point for spawning anything. See
//! `ARCHITECTURE.md` §3. Nothing else in the codebase should call `Command::new`.

pub mod events;
mod line_splitter;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use crate::error::{AppError, Result};
use line_splitter::LineSplitter;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::{mpsc, watch};
use ts_rs::TS;

/// How long `terminate()` waits after the soft signal before escalating to a force-kill.
const TERMINATE_GRACE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct ProcId(pub String);

impl ProcId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl Default for ProcId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ProcKind {
    Pio,
    Claude,
    Installer,
    Tool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum StdStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub stream: StdStream,
    pub text: String,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProcSummary {
    pub id: ProcId,
    pub label: String,
    pub kind: ProcKind,
    pub started_at_ms: u64,
    pub pid: Option<u32>,
}

/// A fully-specified child process invocation. Every field is set by trusted, server-side
/// code — the frontend never constructs one of these or sees an argv array.
pub struct SpawnSpec {
    /// Resolved absolute path. Never a bare name looked up on `PATH` implicitly by the OS.
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Additive on top of the inherited environment; secrets are injected here only.
    pub env: Vec<(String, String)>,
    pub kind: ProcKind,
    pub label: String,
}

pub struct Handle {
    pub id: ProcId,
    pub started_at: Instant,
    /// Resolves to the exit code once the process ends. Callers that only care about the
    /// streamed lines (most of them) can drop this; it costs one `oneshot` channel.
    pub exit_code: tokio::sync::oneshot::Receiver<i32>,
}

pub struct CapturedOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Line-oriented sink a caller passes to `spawn_streaming`. Kept as a plain channel (not a
/// Tauri `Channel<T>`) so `core/proc` stays unit-testable without Tauri — the `commands/`
/// layer adapts this into a coalesced IPC channel.
pub type LineSink = mpsc::UnboundedSender<LogLine>;

/// OS-specific signal delivery for a spawned process tree. Implementations must be safe to
/// call from a synchronous `Drop` — no `.await`.
trait Killer: Send + Sync {
    /// SIGINT (Unix) / CTRL_BREAK (Windows) — asks the tree to end cleanly.
    fn interrupt(&self) -> std::io::Result<()>;
    /// SIGTERM (Unix) — Windows has no graceful equivalent for an arbitrary console tree,
    /// so `WinKiller::terminate` escalates straight to a force-kill.
    fn terminate(&self) -> std::io::Result<()>;
    /// SIGKILL (Unix) / `TerminateJobObject` (Windows) — immediate, unconditional.
    fn force_kill(&self) -> std::io::Result<()>;
}

struct ProcEntry {
    label: String,
    kind: ProcKind,
    started_at_ms: u64,
    pid: Option<u32>,
    killer: Arc<dyn Killer>,
    /// Flips to `true` once the reaper task observes the child exit.
    exited_rx: watch::Receiver<bool>,
}

#[derive(Default)]
pub struct ProcessSupervisor {
    table: Arc<Mutex<HashMap<ProcId, ProcEntry>>>,
}

impl ProcessSupervisor {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn spawn_streaming(&self, spec: SpawnSpec, sink: LineSink) -> Result<Handle> {
        let mut cmd = self.build_command(&spec);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.stdin(Stdio::null());

        let mut child = cmd.spawn().map_err(spawn_err(&spec))?;
        let pid = child.id();

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        if let Some(stdout) = stdout {
            tokio::spawn(pump_stream(stdout, StdStream::Stdout, sink.clone()));
        }
        if let Some(stderr) = stderr {
            tokio::spawn(pump_stream(stderr, StdStream::Stderr, sink));
        }

        let killer = self.make_killer(&child, pid)?;
        let (exited_tx, exited_rx) = watch::channel(false);
        let id = ProcId::new();
        let started_at = Instant::now();
        let started_at_ms = epoch_ms();

        self.table.lock().unwrap().insert(
            id.clone(),
            ProcEntry {
                label: spec.label.clone(),
                kind: spec.kind,
                started_at_ms,
                pid,
                killer,
                exited_rx,
            },
        );

        let (exit_code_tx, exit_code_rx) = tokio::sync::oneshot::channel();
        let table = self.table.clone();
        let reap_id = id.clone();
        tokio::spawn(async move {
            let status = child.wait().await;
            let code = status.ok().and_then(|s| s.code()).unwrap_or(-1);
            let _ = exit_code_tx.send(code);
            let _ = exited_tx.send(true);
            table.lock().unwrap().remove(&reap_id);
        });

        Ok(Handle {
            id,
            started_at,
            exit_code: exit_code_rx,
        })
    }

    pub async fn spawn_capture(&self, spec: SpawnSpec) -> Result<CapturedOutput> {
        let mut cmd = self.build_command(&spec);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.stdin(Stdio::null());

        let child = cmd.spawn().map_err(spawn_err(&spec))?;
        let output = child.wait_with_output().await.map_err(spawn_err(&spec))?;

        Ok(CapturedOutput {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    pub async fn interrupt(&self, id: &ProcId) -> Result<()> {
        self.with_killer(id, |k| k.interrupt())
    }

    /// SIGTERM, then escalates to SIGKILL if the process is still alive after a grace
    /// period. On Windows this collapses to an immediate force-kill (see `WinKiller`).
    pub async fn terminate(&self, id: &ProcId) -> Result<()> {
        let mut exited_rx = {
            let table = self.table.lock().unwrap();
            let entry = table
                .get(id)
                .ok_or_else(|| unknown_proc_err(id))?;
            entry.killer.terminate().map_err(io_err)?;
            entry.exited_rx.clone()
        };

        let already_exited = *exited_rx.borrow();
        if already_exited {
            return Ok(());
        }
        let waited = tokio::time::timeout(TERMINATE_GRACE, exited_rx.changed()).await;
        if waited.is_err() {
            // Still alive after the grace period — force-kill. The entry may already be
            // gone if it exited in the tiny window right after the timeout fired.
            if let Some(entry) = self.table.lock().unwrap().get(id) {
                entry.killer.force_kill().map_err(io_err)?;
            }
        }
        Ok(())
    }

    pub fn running(&self) -> Vec<ProcSummary> {
        self.table
            .lock()
            .unwrap()
            .iter()
            .map(|(id, e)| ProcSummary {
                id: id.clone(),
                label: e.label.clone(),
                kind: e.kind,
                started_at_ms: e.started_at_ms,
                pid: e.pid,
            })
            .collect()
    }

    /// Best-effort synchronous kill of every tracked process. Safe to call from `Drop` and
    /// from the Tauri `RunEvent::Exit` hook — does not wait for exit confirmation.
    pub fn kill_all(&self) {
        let table = self.table.lock().unwrap();
        for entry in table.values() {
            let _ = entry.killer.terminate();
        }
    }

    fn with_killer(&self, id: &ProcId, f: impl FnOnce(&dyn Killer) -> std::io::Result<()>) -> Result<()> {
        let table = self.table.lock().unwrap();
        let entry = table.get(id).ok_or_else(|| unknown_proc_err(id))?;
        f(entry.killer.as_ref()).map_err(io_err)
    }

    fn build_command(&self, spec: &SpawnSpec) -> Command {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args);
        cmd.current_dir(&spec.cwd);
        cmd.envs(spec.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        cmd.kill_on_drop(false); // we own lifecycle via the process table, not tokio's Drop.

        #[cfg(unix)]
        unix::prepare(&mut cmd);
        #[cfg(windows)]
        windows::prepare(&mut cmd);

        cmd
    }

    #[cfg(unix)]
    fn make_killer(&self, _child: &tokio::process::Child, pid: Option<u32>) -> Result<Arc<dyn Killer>> {
        let pid = pid.ok_or_else(|| AppError::Io {
            message: "spawned process has no pid".into(),
        })?;
        Ok(Arc::new(unix::UnixKiller::new(pid)))
    }

    #[cfg(windows)]
    fn make_killer(&self, child: &tokio::process::Child, pid: Option<u32>) -> Result<Arc<dyn Killer>> {
        let pid = pid.ok_or_else(|| AppError::Io {
            message: "spawned process has no pid".into(),
        })?;
        let job = windows::assign_to_new_job(child).map_err(io_err)?;
        Ok(Arc::new(windows::WinKiller::new(pid, job)))
    }
}

impl Drop for ProcessSupervisor {
    fn drop(&mut self) {
        self.kill_all();
    }
}

async fn pump_stream(
    mut reader: impl tokio::io::AsyncRead + Unpin,
    stream: StdStream,
    sink: LineSink,
) {
    let mut splitter = LineSplitter::new();
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                for line in splitter.feed(&buf[..n]) {
                    if sink
                        .send(LogLine {
                            stream,
                            text: line,
                            ts_ms: epoch_ms(),
                        })
                        .is_err()
                    {
                        return; // receiver gone — nothing left to do
                    }
                }
            }
            Err(_) => break,
        }
    }
    if let Some(line) = splitter.finish() {
        let _ = sink.send(LogLine {
            stream,
            text: line,
            ts_ms: epoch_ms(),
        });
    }
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn spawn_err(spec: &SpawnSpec) -> impl Fn(std::io::Error) -> AppError + '_ {
    move |e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::ToolMissing {
                tool: spec.program.display().to_string(),
                install_action: false,
            }
        } else {
            AppError::Io {
                message: format!("spawning {}: {e}", spec.program.display()),
            }
        }
    }
}

fn io_err(e: std::io::Error) -> AppError {
    AppError::Io {
        message: e.to_string(),
    }
}

fn unknown_proc_err(id: &ProcId) -> AppError {
    AppError::Io {
        message: format!("no tracked process with id {}", id.0),
    }
}
