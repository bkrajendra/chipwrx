//! The turn runner: spawns `claude -p ...`, feeds its stdout through
//! [`ndjson::NdjsonParser`], and forwards `ChatEvent`s — deltas coalesced on a ~16 ms tick
//! (`ARCHITECTURE.md` §8), everything else immediately. See `ARCHITECTURE.md` §3
//! `core/claude`.

use super::argv::{build_turn_args, SessionRef};
use super::ids::TurnId;
use super::ndjson::{Coalescer, NdjsonParser};
use super::types::ChatEvent;
use crate::core::proc::{ProcId, ProcKind, ProcessSupervisor, SpawnSpec, StdStream};
use crate::core::redact::redact;
use crate::core::settings::PermissionPolicySetting;
use crate::core::toolchain::resolve::Resolution;
use crate::error::{AppError, Result};
use std::path::Path;
use tokio::sync::mpsc;

/// Coalescing tick — same cadence as `core::proc::events::spawn_with_proc_events`
/// (`ARCHITECTURE.md` §8).
const COALESCE_TICK: std::time::Duration = std::time::Duration::from_millis(16);
/// `PioCommandFailed.tail`'s sibling for `ClaudeProcessFailed` — last ~4 KB of stderr.
const STDERR_TAIL_CAP: usize = 4096;
/// Exit code the Claude CLI uses for a clean `SIGTERM` (`CLI-CONTRACT.md` §1.4 signals
/// table).
const SIGTERM_EXIT_CODE: i32 = 143;

pub struct RunTurnRequest<'a> {
    pub claude: &'a Resolution,
    pub workspace: &'a Path,
    pub turn_id: TurnId,
    pub prompt: &'a str,
    pub session: SessionRef,
    pub policy: PermissionPolicySetting,
    pub model: &'a str,
    pub permission_prompts_none_supported: bool,
    /// Additive child-process env — `NFR-S1`: the optional `ANTHROPIC_API_KEY` from the OS
    /// keychain, injected here only (never written to argv, never logged unredacted).
    pub env: Vec<(String, String)>,
}

/// Spawns the turn and returns immediately with its `ProcId` (for `claude_stop_turn`);
/// `ChatEvent`s stream to `sink` from a background task for the lifetime of the process.
pub async fn run_turn(
    supervisor: &ProcessSupervisor,
    req: RunTurnRequest<'_>,
    sink: mpsc::UnboundedSender<ChatEvent>,
) -> Result<ProcId> {
    let mut args = req.claude.extra_args.clone();
    args.extend(build_turn_args(
        req.prompt,
        req.policy,
        req.permission_prompts_none_supported,
        req.model,
        &req.session,
    ));

    let spec = SpawnSpec {
        program: req.claude.program.clone(),
        args,
        cwd: req.workspace.to_path_buf(),
        env: req.env,
        kind: ProcKind::Claude,
        label: "claude-turn".into(),
    };

    let (line_tx, mut line_rx) = mpsc::unbounded_channel();
    let handle = supervisor.spawn_streaming(spec, line_tx).await?;
    let proc_id = handle.id.clone();
    let turn_id = req.turn_id;
    let session_id_for_interrupt = req.session.id().to_string();

    tokio::spawn(async move {
        let mut parser = NdjsonParser::new(turn_id.clone());
        let mut coalescer = Coalescer::new();
        let mut ticker = tokio::time::interval(COALESCE_TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut result_seen = false;
        let mut stderr_tail = String::new();

        loop {
            tokio::select! {
                line = line_rx.recv() => match line {
                    Some(l) => match l.stream {
                        StdStream::Stdout => {
                            for ev in parser.feed_line(&l.text, &mut coalescer) {
                                if matches!(ev, ChatEvent::Result { .. }) {
                                    result_seen = true;
                                }
                                let _ = sink.send(ev);
                            }
                        }
                        StdStream::Stderr => append_tail(&mut stderr_tail, &l.text),
                    },
                    None => break, // both pipes hit EOF
                },
                _ = ticker.tick() => {
                    if !coalescer.is_empty() {
                        for ev in coalescer.flush(&turn_id) {
                            let _ = sink.send(ev);
                        }
                    }
                }
            }
        }
        if !coalescer.is_empty() {
            for ev in coalescer.flush(&turn_id) {
                let _ = sink.send(ev);
            }
        }

        let exit_code = handle.exit_code.await.unwrap_or(-1);
        if !result_seen {
            let error = classify_failure(exit_code, &session_id_for_interrupt, &stderr_tail);
            let _ = sink.send(ChatEvent::Failed { turn_id, error });
        }
    });

    Ok(proc_id)
}

/// A turn's process exited without ever producing a parseable `result` line
/// (`result_seen == false`). `FR-CHAT-5`: exit 143 is `SIGTERM`'s signature — a clean
/// interruption the UI must describe as "will resume", never a generic error.
fn classify_failure(exit_code: i32, session_id: &str, stderr_tail: &str) -> AppError {
    if exit_code == SIGTERM_EXIT_CODE {
        AppError::ClaudeInterrupted {
            session_id: session_id.to_string(),
        }
    } else {
        AppError::ClaudeProcessFailed {
            exit_code,
            tail: redact(stderr_tail, false),
        }
    }
}

fn append_tail(tail: &mut String, line: &str) {
    tail.push_str(line);
    tail.push('\n');
    if tail.len() > STDERR_TAIL_CAP {
        let start = tail.len() - STDERR_TAIL_CAP;
        // Don't split a multi-byte UTF-8 char in half.
        let start = (start..tail.len()).find(|&i| tail.is_char_boundary(i)).unwrap_or(tail.len());
        *tail = tail[start..].to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_tail_caps_at_the_byte_limit() {
        let mut tail = String::new();
        for i in 0..2000 {
            append_tail(&mut tail, &format!("line {i}"));
        }
        assert!(tail.len() <= STDERR_TAIL_CAP);
        assert!(tail.ends_with("line 1999\n"));
    }

    #[test]
    fn exit_143_classifies_as_a_clean_interruption() {
        match classify_failure(SIGTERM_EXIT_CODE, "s1", "") {
            AppError::ClaudeInterrupted { session_id } => assert_eq!(session_id, "s1"),
            other => panic!("expected ClaudeInterrupted, got {other:?}"),
        }
    }

    #[test]
    fn any_other_exit_code_classifies_as_a_process_failure_with_redacted_tail() {
        match classify_failure(1, "s1", "ANTHROPIC_API_KEY=sk-ant-secretvalue\n") {
            AppError::ClaudeProcessFailed { exit_code, tail } => {
                assert_eq!(exit_code, 1);
                assert!(!tail.contains("secretvalue"));
            }
            other => panic!("expected ClaudeProcessFailed, got {other:?}"),
        }
    }
}
