//! `ProcEvent` — the streamed payload for long-running `pio`-backed operations
//! (`pio project init` now; `pio run` builds/uploads in M5). See `IPC-CONTRACT.md` §5.
//!
//! `Defect`/`SizeUsage` are defined here (not populated until `core/diag` exists in M5) so
//! this enum's shape is settled now and every consumer of it compiles against the same
//! type from the start.

use super::{LogLine, ProcId, ProcessSupervisor, SpawnSpec};
use crate::error::Result;
use serde::Serialize;
use std::time::Duration;
use tokio::sync::mpsc;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Error,
    Warning,
    Note,
}

#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum DefectSource {
    Compiler,
    Linker,
    Check,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Defect {
    pub file: String,
    pub line: u32,
    pub column: Option<u32>,
    pub severity: Severity,
    pub message: String,
    pub source: DefectSource,
    pub raw: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SizeUsage {
    pub ram_used: u64,
    pub ram_total: u64,
    pub flash_used: u64,
    pub flash_total: u64,
    pub ram_delta: Option<i64>,
    pub flash_delta: Option<i64>,
}

/// `M9`/`FR-BUILD-10`: statuses `pio test --json-output` actually emits (verified against a
/// real run — `tests/fixtures/pio-test-json-real.json` — for `ERRORED`; `PASSED`/`FAILED`/
/// `SKIPPED` are PlatformIO's own well-known other values for the same field, not
/// independently captured here).
#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TestStatus {
    Passed,
    Failed,
    Errored,
    Skipped,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TestCaseResult {
    pub name: String,
    pub status: TestStatus,
    /// The assertion failure text, or the build/run exception — whichever `pio test` gave.
    pub message: Option<String>,
    pub duration: f64,
    pub file: Option<String>,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TestSuite {
    pub env_name: String,
    pub test_name: String,
    pub status: TestStatus,
    pub duration: f64,
    pub cases: Vec<TestCaseResult>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "type", content = "data")]
pub enum ProcEvent {
    Started {
        proc_id: ProcId,
        label: String,
        argv: Vec<String>,
    },
    /// Batched: up to N lines per message (`ARCHITECTURE.md` §8) — coalesced in Rust, not
    /// forwarded one IPC message per line.
    Lines {
        proc_id: ProcId,
        lines: Vec<LogLine>,
    },
    Defect {
        proc_id: ProcId,
        defect: Defect,
    },
    Size {
        proc_id: ProcId,
        usage: SizeUsage,
    },
    /// `M9`/`FR-BUILD-10`: one per environment `pio test --json-output` reports on —
    /// synthesized from the single JSON blob it prints at the end of the run, the same way
    /// `Defect` events are synthesized from `pio check --json-output`'s. Not in
    /// `IPC-CONTRACT.md`'s original `ProcEvent` sketch (`SPEC.md` §8 open question 39): that
    /// sketch predates M9 and has no variant carrying a structured pass/fail list, which
    /// `FR-BUILD-10` requires — this is an additive extension, not a divergence.
    TestResult {
        proc_id: ProcId,
        suite: TestSuite,
    },
    Stage {
        proc_id: ProcId,
        stage: String,
    },
    Finished {
        proc_id: ProcId,
        success: bool,
        exit_code: i32,
        duration_ms: u64,
    },
}

/// Coalescing tick for batching lines into `ProcEvent::Lines` (`ARCHITECTURE.md` §8) — one
/// IPC message carrying many lines beats one message per line.
const COALESCE_TICK: Duration = Duration::from_millis(16);

/// Spawns `spec`, emits `Started` immediately, streams its output as coalesced `Lines`
/// events, and emits `Finished` once it exits — all on a background task, so this returns
/// as soon as the process is spawned rather than waiting for it to finish. The returned
/// `ProcId` can be passed to `ProcessSupervisor::interrupt`/`terminate` to cancel it.
pub async fn spawn_with_proc_events(
    supervisor: &ProcessSupervisor,
    spec: SpawnSpec,
    sink: mpsc::UnboundedSender<ProcEvent>,
) -> Result<ProcId> {
    let mut argv = vec![spec.program.display().to_string()];
    argv.extend(spec.args.iter().cloned());
    let label = spec.label.clone();

    let (line_tx, mut line_rx) = mpsc::unbounded_channel();
    let handle = supervisor.spawn_streaming(spec, line_tx).await?;
    let proc_id = handle.id.clone();

    let _ = sink.send(ProcEvent::Started {
        proc_id: proc_id.clone(),
        label,
        argv,
    });

    let started_at = handle.started_at;
    let event_id = proc_id.clone();
    tokio::spawn(async move {
        let mut buf: Vec<LogLine> = Vec::new();
        let mut ticker = tokio::time::interval(COALESCE_TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                line = line_rx.recv() => match line {
                    Some(l) => buf.push(l),
                    None => break, // both pipes hit EOF
                },
                _ = ticker.tick() => {
                    if !buf.is_empty() {
                        let _ = sink.send(ProcEvent::Lines { proc_id: event_id.clone(), lines: std::mem::take(&mut buf) });
                    }
                }
            }
        }
        if !buf.is_empty() {
            let _ = sink.send(ProcEvent::Lines { proc_id: event_id.clone(), lines: buf });
        }

        let exit_code = handle.exit_code.await.unwrap_or(-1);
        let _ = sink.send(ProcEvent::Finished {
            proc_id: event_id,
            success: exit_code == 0,
            exit_code,
            duration_ms: started_at.elapsed().as_millis() as u64,
        });
    });

    Ok(proc_id)
}
