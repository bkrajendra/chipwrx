//! Integration tests for `core::claude::turn::run_turn` against `FakeCli` standing in for
//! `claude`. See `ROADMAP.md` M3's acceptance test: "against the `FakeCli` transcript, a
//! turn streams token-by-token at frame rate, tool cards appear in order, Stop halts
//! mid-stream and the UI says the turn was interrupted."

use std::path::PathBuf;
use vibe_hardware_lib::core::claude::argv::SessionRef;
use vibe_hardware_lib::core::claude::ids::TurnId;
use vibe_hardware_lib::core::claude::turn::{run_turn, RunTurnRequest};
use vibe_hardware_lib::core::claude::types::ChatEvent;
use vibe_hardware_lib::core::proc::ProcessSupervisor;
use vibe_hardware_lib::core::settings::PermissionPolicySetting;
use vibe_hardware_lib::core::toolchain::resolve::{Resolution, Source};

fn fake_claude() -> Resolution {
    Resolution {
        program: PathBuf::from(env!("CARGO_BIN_EXE_fake_cli")),
        extra_args: vec![],
        source: Source::Setting,
    }
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vibe-hw-claude-turn-test-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn set_fixture(dir: &std::path::Path, fixture_path: &std::path::Path, interval_ms: Option<u64>) {
    let marker = match interval_ms {
        Some(ms) => format!("{}\n{ms}", fixture_path.display()),
        None => fixture_path.display().to_string(),
    };
    std::fs::write(dir.join(".fake-cli-fixture"), marker).expect("write marker file");
}

fn repo_fixture(name: &str) -> PathBuf {
    // `CARGO_MANIFEST_DIR` is `src-tauri/`; the shared golden fixtures live one level up.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("tests").join("fixtures").join(name)
}

#[tokio::test]
async fn full_transcript_streams_events_in_order_and_ends_with_a_result() {
    let dir = scratch_dir("full");
    set_fixture(&dir, &repo_fixture("claude-turn.ndjson"), None);

    let supervisor = ProcessSupervisor::new();
    let claude = fake_claude();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let turn_id = TurnId::new();

    run_turn(
        &supervisor,
        RunTurnRequest {
            claude: &claude,
            workspace: &dir,
            turn_id: turn_id.clone(),
            prompt: "add a blink sketch",
            session: SessionRef::New(uuid::Uuid::new_v4().to_string()),
            policy: PermissionPolicySetting::Guarded,
            model: "sonnet",
            permission_prompts_none_supported: true,
        },
        tx,
    )
    .await
    .expect("spawn");

    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }

    assert!(matches!(events.first(), Some(ChatEvent::SessionReady { .. })), "expected SessionReady first, got {:?}", events.first());

    let tool_started_idx = events.iter().position(|e| matches!(e, ChatEvent::ToolCallStarted { .. }));
    let tool_completed_idx = events.iter().position(|e| matches!(e, ChatEvent::ToolCallCompleted { .. }));
    let tool_result_idx = events.iter().position(|e| matches!(e, ChatEvent::ToolResult { .. }));
    let result_idx = events.iter().position(|e| matches!(e, ChatEvent::Result { .. }));

    let (started, completed, result_tool, result) = (
        tool_started_idx.expect("ToolCallStarted"),
        tool_completed_idx.expect("ToolCallCompleted"),
        tool_result_idx.expect("ToolResult"),
        result_idx.expect("Result"),
    );
    assert!(started < completed, "tool card must start before it completes");
    assert!(completed < result_tool, "the tool call completes before its result arrives");
    assert!(result_tool < result, "the turn result is the final event");

    // No TextDelta arrives *after* its TextBlock — the coalescer flush ordering guarantee.
    let text_block_idx = events.iter().position(|e| matches!(e, ChatEvent::TextBlock { .. })).expect("TextBlock");
    let last_text_delta_idx = events.iter().rposition(|e| matches!(e, ChatEvent::TextDelta { .. }));
    if let Some(last_delta) = last_text_delta_idx {
        assert!(last_delta < text_block_idx, "all text deltas for a block must precede its TextBlock");
    }

    assert!(!events.iter().any(|e| matches!(e, ChatEvent::Failed { .. })), "a clean run must not emit Failed");
    assert!(matches!(events.last(), Some(ChatEvent::Result { .. })), "Result must be the last event");
}

#[tokio::test]
async fn stop_mid_stream_terminates_the_process_and_reports_failure_not_a_result() {
    let dir = scratch_dir("stop");
    // A long-running fixture so there's time to cancel mid-stream.
    let mut content = String::new();
    for i in 0..500 {
        content.push_str(&format!(
            r#"{{"type":"stream_event","event":{{"type":"content_block_delta","index":0,"delta":{{"type":"text_delta","text":"line {i} "}}}}}}"#
        ));
        content.push('\n');
    }
    let fixture_path = dir.join("slow.ndjson");
    std::fs::write(&fixture_path, content).unwrap();
    set_fixture(&dir, &fixture_path, Some(5));

    let supervisor = ProcessSupervisor::new();
    let claude = fake_claude();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let turn_id = TurnId::new();

    let proc_id = run_turn(
        &supervisor,
        RunTurnRequest {
            claude: &claude,
            workspace: &dir,
            turn_id: turn_id.clone(),
            prompt: "do something slow",
            session: SessionRef::New(uuid::Uuid::new_v4().to_string()),
            policy: PermissionPolicySetting::Guarded,
            model: "sonnet",
            permission_prompts_none_supported: true,
        },
        tx,
    )
    .await
    .expect("spawn");

    // Let a little output through, then stop it — mirrors `claude_stop_turn`'s SIGTERM path
    // (`hard: true`), which is what's actually observable cross-platform: Windows has no
    // graceful SIGTERM equivalent, so `terminate()` escalates straight to a force-kill
    // there (`core::proc` doc comment) — this test asserts the *outcome* (no Result, no
    // surviving process, a Failed event) rather than a specific exit code.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    supervisor.terminate(&proc_id).await.expect("terminate");

    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }

    assert!(!events.iter().any(|e| matches!(e, ChatEvent::Result { .. })), "an interrupted turn must not report a Result");
    match events.last() {
        Some(ChatEvent::Failed { .. }) => {}
        other => panic!("expected the turn to end with Failed, got {other:?}"),
    }
    assert!(!supervisor.running().iter().any(|p| p.id == proc_id), "the process must be gone after Stop");
}
