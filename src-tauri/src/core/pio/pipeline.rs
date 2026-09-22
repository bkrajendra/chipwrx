//! The build/upload pipeline: the `Idle → Building → BuildOk/Failed → Uploading` half of
//! `ARCHITECTURE.md` §4.1's state machine (`SPEC.md` §8 open question 18 — the
//! `Thinking`/`Writing` chat half isn't wired here), and [`spawn_pio_run`], the
//! diagnostic/size-aware `pio run` spawn primitive `commands::pipeline` builds
//! `pipeline_build`/`pipeline_upload`/`pipeline_run_target` on top of.
//!
//! [`spawn_pio_run`] duplicates the coalescing-tick shape of
//! `core::proc::events::spawn_with_proc_events` and `core::claude::turn::run_turn` rather
//! than sharing it — each needs different per-line handling (plain lines vs. NDJSON vs.
//! diagnostics), and by M3 this was already the established pattern rather than a
//! generic hook threaded through the shared one.

use super::watch;
use crate::core::diag::parser::DiagnosticParser;
use crate::core::diag::size::SizeParser;
use crate::core::proc::events::ProcEvent;
use crate::core::proc::{ProcId, ProcKind, ProcessSupervisor, SpawnSpec};
use crate::core::settings::PipelinePolicySetting;
use crate::core::toolchain::resolve::Resolution;
use crate::error::Result;
use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;
use tokio::sync::mpsc;

const COALESCE_TICK: std::time::Duration = std::time::Duration::from_millis(16);

// ---------------------------------------------------------------------------------------
// spawn_pio_run — the diagnostic/size-aware `pio run` primitive
// ---------------------------------------------------------------------------------------

/// Spawns `pio` with `args` (already including `run` and every flag — see
/// `core::pio::run`), streaming coalesced `Lines` plus discrete `Defect`/`Size`/`Stage`
/// events as they're recognized, and a final `Finished`. Returns as soon as the process is
/// spawned; everything else happens on a background task.
pub async fn spawn_pio_run(
    supervisor: &ProcessSupervisor,
    pio: &Resolution,
    workspace: &Path,
    args: Vec<String>,
    label: &str,
    sink: mpsc::UnboundedSender<ProcEvent>,
) -> Result<ProcId> {
    let mut full_args = pio.extra_args.clone();
    full_args.extend(args);

    let spec = SpawnSpec {
        program: pio.program.clone(),
        args: full_args.clone(),
        cwd: workspace.to_path_buf(),
        env: vec![],
        kind: ProcKind::Pio,
        label: label.to_string(),
    };

    let (line_tx, mut line_rx) = mpsc::unbounded_channel();
    let handle = supervisor.spawn_streaming(spec, line_tx).await?;
    let proc_id = handle.id.clone();

    let mut argv = vec![pio.program.display().to_string()];
    argv.extend(full_args);
    let _ = sink.send(ProcEvent::Started {
        proc_id: proc_id.clone(),
        label: label.to_string(),
        argv,
    });

    let started_at = handle.started_at;
    let event_id = proc_id.clone();
    tokio::spawn(async move {
        let mut diag = DiagnosticParser::new();
        let mut size = SizeParser::new();
        let mut buf = Vec::new();
        let mut ticker = tokio::time::interval(COALESCE_TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                line = line_rx.recv() => match line {
                    Some(l) => {
                        // Diagnostics/size are only meaningful on stdout+stderr text, same
                        // as the raw lines themselves — no stream filtering needed here,
                        // `pio run` interleaves compiler stderr into the combined stream.
                        if let Some(defect) = diag.feed_line(&l.text) {
                            let _ = sink.send(ProcEvent::Defect { proc_id: event_id.clone(), defect });
                        }
                        if let Some(usage) = size.feed_line(&l.text) {
                            let _ = sink.send(ProcEvent::Size { proc_id: event_id.clone(), usage });
                        }
                        if let Some(stage) = detect_stage(&l.text) {
                            let _ = sink.send(ProcEvent::Stage { proc_id: event_id.clone(), stage });
                        }
                        buf.push(l);
                    }
                    None => break,
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

/// `CLI-CONTRACT.md` §5.1: "Terminal status lines to key the UI off... the `Building
/// .pio/build/<env>/firmware.bin` / `Writing at 0x...` progress lines during upload."
/// `Writing at 0x...` is esptool output during a real flash — unverified against a real
/// capture in this environment (no hardware), included per CLI-CONTRACT's own text.
const STAGE_PREFIXES: &[&str] = &[
    "Compiling ",
    "Linking ",
    "Building ",
    "Archiving ",
    "Generating ",
    "Writing at ",
    "Checking size ",
    "Retrieving maximum program size ",
];

fn detect_stage(line: &str) -> Option<String> {
    let trimmed = line.trim();
    STAGE_PREFIXES.iter().any(|p| trimmed.starts_with(p)).then(|| trimmed.to_string())
}

// ---------------------------------------------------------------------------------------
// Pipeline state (ARCHITECTURE.md §4.1, build/upload half only — SPEC.md §8 open
// question 18)
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, ts_rs::TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PipelineStep {
    Idle,
    Building,
    BuildOk,
    Uploading,
    Failed,
    /// Reserved for M6 (`PortBroker`'s monitor) — never entered by this milestone's code.
    Monitoring,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ts_rs::TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PipelineState {
    pub step: PipelineStep,
    /// RFC3339 — when `step` was entered.
    pub since: String,
    pub env: Option<String>,
    pub proc_id: Option<ProcId>,
}

impl PipelineState {
    fn idle() -> Self {
        Self {
            step: PipelineStep::Idle,
            since: chrono::Utc::now().to_rfc3339(),
            env: None,
            proc_id: None,
        }
    }
}

struct Entry {
    state: PipelineState,
    /// Recorded when `BuildOk` is entered — the Safe-policy staleness check compares this
    /// against `watch::max_watched_mtime` on demand (`FR-BUILD-3`).
    build_ok_mtime: Option<SystemTime>,
}

/// Per-workspace pipeline state, in memory only (not persisted — a fresh launch always
/// starts every workspace at `Idle`, which is correct: nothing is actually running).
#[derive(Default)]
pub struct PipelineRegistry {
    entries: HashMap<String, Entry>,
}

impl PipelineRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, workspace_id: &str) -> PipelineState {
        self.entries.get(workspace_id).map(|e| e.state.clone()).unwrap_or_else(PipelineState::idle)
    }

    pub fn transition(&mut self, workspace_id: &str, step: PipelineStep, env: Option<String>, proc_id: Option<ProcId>) -> PipelineState {
        let state = PipelineState {
            step,
            since: chrono::Utc::now().to_rfc3339(),
            env,
            proc_id,
        };
        let build_ok_mtime = None;
        self.entries.insert(
            workspace_id.to_string(),
            Entry {
                state: state.clone(),
                build_ok_mtime,
            },
        );
        state
    }

    /// Enters `BuildOk`, recording the current watched-file mtime for later staleness
    /// checks.
    pub fn mark_build_ok(&mut self, workspace_id: &str, workspace_path: &Path, env: &str) -> PipelineState {
        let state = PipelineState {
            step: PipelineStep::BuildOk,
            since: chrono::Utc::now().to_rfc3339(),
            env: Some(env.to_string()),
            proc_id: None,
        };
        self.entries.insert(
            workspace_id.to_string(),
            Entry {
                state: state.clone(),
                build_ok_mtime: watch::max_watched_mtime(workspace_path),
            },
        );
        state
    }

    /// Returns the current state, first downgrading `BuildOk` back to `Idle` in place if
    /// `policy` is Safe and a watched file has changed since the build that earned it
    /// (`FR-BUILD-3`).
    pub fn effective_state(&mut self, workspace_id: &str, workspace_path: &Path, policy: PipelinePolicySetting) -> PipelineState {
        let stale = matches!(policy, PipelinePolicySetting::Safe)
            && self.entries.get(workspace_id).is_some_and(|e| {
                e.state.step == PipelineStep::BuildOk
                    && match (e.build_ok_mtime, watch::max_watched_mtime(workspace_path)) {
                        (Some(recorded), Some(now)) => now > recorded,
                        (None, Some(_)) => true, // watched files appeared after an empty build
                        _ => false,
                    }
            });
        if stale {
            return self.transition(workspace_id, PipelineStep::Idle, None, None);
        }
        self.get(workspace_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_known_stage_prefixes() {
        assert_eq!(detect_stage("Compiling .pio/build/esp32dev/src/main.cpp.o"), Some("Compiling .pio/build/esp32dev/src/main.cpp.o".to_string()));
        assert_eq!(detect_stage("Linking .pio/build/esp32dev/firmware.elf"), Some("Linking .pio/build/esp32dev/firmware.elf".to_string()));
        assert_eq!(detect_stage("Writing at 0x00010000... (12 %)"), Some("Writing at 0x00010000... (12 %)".to_string()));
    }

    #[test]
    fn ignores_unrelated_lines() {
        assert_eq!(detect_stage("PLATFORM: Espressif 32 (55.3.311)"), None);
        assert_eq!(detect_stage(""), None);
    }

    #[test]
    fn fresh_workspace_starts_idle() {
        let reg = PipelineRegistry::new();
        assert_eq!(reg.get("ws-1").step, PipelineStep::Idle);
    }

    #[test]
    fn transition_updates_and_persists_state() {
        let mut reg = PipelineRegistry::new();
        let state = reg.transition("ws-1", PipelineStep::Building, Some("esp32dev".into()), Some(ProcId("p1".into())));
        assert_eq!(state.step, PipelineStep::Building);
        assert_eq!(reg.get("ws-1").step, PipelineStep::Building);
    }

    #[test]
    fn safe_policy_invalidates_build_ok_after_a_watched_file_changes() {
        let ws = std::env::temp_dir().join(format!("vibe-hw-pipeline-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(ws.join("src")).unwrap();
        std::fs::write(ws.join("src/main.cpp"), "v1").unwrap();

        let mut reg = PipelineRegistry::new();
        reg.mark_build_ok("ws-1", &ws, "esp32dev");
        assert_eq!(reg.effective_state("ws-1", &ws, PipelinePolicySetting::Safe).step, PipelineStep::BuildOk);

        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(ws.join("src/main.cpp"), "v2 - claude's edit").unwrap();

        let state = reg.effective_state("ws-1", &ws, PipelinePolicySetting::Safe);
        assert_eq!(state.step, PipelineStep::Idle, "a watched-file edit must invalidate BuildOk under Safe policy");
    }

    #[test]
    fn fast_path_policy_never_invalidates_build_ok() {
        let ws = std::env::temp_dir().join(format!("vibe-hw-pipeline-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(ws.join("src")).unwrap();
        std::fs::write(ws.join("src/main.cpp"), "v1").unwrap();

        let mut reg = PipelineRegistry::new();
        reg.mark_build_ok("ws-1", &ws, "esp32dev");
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(ws.join("src/main.cpp"), "v2").unwrap();

        let state = reg.effective_state("ws-1", &ws, PipelinePolicySetting::FastPath);
        assert_eq!(state.step, PipelineStep::BuildOk, "Fast-path implies build-with-upload, so staleness doesn't matter the same way");
    }
}
