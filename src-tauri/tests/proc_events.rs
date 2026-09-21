//! Integration tests for `core::proc::events::spawn_with_proc_events` — the coalescing
//! ProcEvent runner shared by `project_create` (M2) and the build pipeline (M5).

use std::path::PathBuf;
use vibe_hardware_lib::core::proc::events::{spawn_with_proc_events, ProcEvent};
use vibe_hardware_lib::core::proc::{ProcKind, ProcessSupervisor, SpawnSpec};

fn fake_cli_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake_cli"))
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vibe-hw-procevents-test-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn set_fixture(dir: &std::path::Path, content: &str) {
    let fixture_path = dir.join("fixture.txt");
    std::fs::write(&fixture_path, content).expect("write fixture");
    std::fs::write(dir.join(".fake-cli-fixture"), fixture_path.display().to_string())
        .expect("write marker file");
}

#[tokio::test]
async fn emits_started_lines_and_finished_in_order() {
    let dir = scratch_dir("basic");
    set_fixture(&dir, "line one\nline two\nline three\n");

    let supervisor = ProcessSupervisor::new();
    let spec = SpawnSpec {
        program: fake_cli_path(),
        args: vec![],
        cwd: dir.clone(),
        env: vec![],
        kind: ProcKind::Pio,
        label: "test-init".into(),
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let proc_id = spawn_with_proc_events(&supervisor, spec, tx).await.expect("spawn");

    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }

    match &events[0] {
        ProcEvent::Started { proc_id: id, label, argv } => {
            assert_eq!(id, &proc_id);
            assert_eq!(label, "test-init");
            assert!(argv[0].ends_with("fake_cli") || argv[0].ends_with("fake_cli.exe"));
        }
        other => panic!("expected Started first, got {other:?}"),
    }

    let all_lines: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            ProcEvent::Lines { lines, .. } => Some(lines.iter().map(|l| l.text.clone())),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(all_lines, vec!["line one", "line two", "line three"]);

    match events.last() {
        Some(ProcEvent::Finished { success, exit_code, .. }) => {
            assert!(success);
            assert_eq!(*exit_code, 0);
        }
        other => panic!("expected Finished last, got {other:?}"),
    }
}

#[tokio::test]
async fn reports_nonzero_exit_as_unsuccessful_finish() {
    let dir = scratch_dir("failure");
    let fixture_path = dir.join("fixture.txt");
    std::fs::write(&fixture_path, "uh oh\n").unwrap();
    std::fs::write(dir.join(".fake-cli-fixture"), format!("{}\n0\n1", fixture_path.display())).unwrap();

    let supervisor = ProcessSupervisor::new();
    let spec = SpawnSpec {
        program: fake_cli_path(),
        args: vec![],
        cwd: dir,
        env: vec![],
        kind: ProcKind::Pio,
        label: "test-fail".into(),
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    spawn_with_proc_events(&supervisor, spec, tx).await.expect("spawn");

    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }
    match events.last() {
        Some(ProcEvent::Finished { success, exit_code, .. }) => {
            assert!(!success);
            assert_eq!(*exit_code, 1);
        }
        other => panic!("expected Finished last, got {other:?}"),
    }
}

#[tokio::test]
async fn cancelling_via_the_returned_proc_id_produces_a_finished_event() {
    let dir = scratch_dir("cancel");
    let mut content = String::new();
    for i in 0..5000 {
        content.push_str(&format!("line {i}\n"));
    }
    let fixture_path = dir.join("fixture.txt");
    std::fs::write(&fixture_path, content).unwrap();
    std::fs::write(dir.join(".fake-cli-fixture"), format!("{}\n1", fixture_path.display())).unwrap();

    let supervisor = ProcessSupervisor::new();
    let spec = SpawnSpec {
        program: fake_cli_path(),
        args: vec![],
        cwd: dir,
        env: vec![],
        kind: ProcKind::Pio,
        label: "test-cancel".into(),
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let proc_id = spawn_with_proc_events(&supervisor, spec, tx).await.expect("spawn");

    // Let a little output through, then cancel mid-stream.
    let _ = rx.recv().await;
    supervisor.terminate(&proc_id).await.expect("terminate");

    let mut saw_finished = false;
    while let Some(ev) = rx.recv().await {
        if let ProcEvent::Finished { success, .. } = ev {
            assert!(!success, "a terminated process should not report success");
            saw_finished = true;
        }
    }
    assert!(saw_finished, "expected a Finished event after cancellation");
    assert!(!supervisor.running().iter().any(|p| p.id == proc_id));
}
