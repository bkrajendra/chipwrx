//! Integration tests for `core::pio::pipeline::spawn_pio_run` against `FakeCli` replaying
//! the real captured `pio run` fixtures — end to end through the spawn + coalescing +
//! diagnostic/size parsing pipeline, not just the unit-level parser tests.

use std::path::PathBuf;
use vibe_hardware_lib::core::pio::pipeline::spawn_pio_run;
use vibe_hardware_lib::core::proc::events::ProcEvent;
use vibe_hardware_lib::core::proc::ProcessSupervisor;
use vibe_hardware_lib::core::toolchain::resolve::{Resolution, Source};

fn fake_pio() -> Resolution {
    Resolution {
        program: PathBuf::from(env!("CARGO_BIN_EXE_fake_cli")),
        extra_args: vec![],
        source: Source::Setting,
    }
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vibe-hw-pipeline-run-test-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn set_fixture(dir: &std::path::Path, fixture_path: &std::path::Path, exit_code: Option<u8>) {
    let marker = match exit_code {
        Some(code) => format!("{}\n0\n{code}", fixture_path.display()),
        None => fixture_path.display().to_string(),
    };
    std::fs::write(dir.join(".fake-cli-fixture"), marker).expect("write marker file");
}

fn repo_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("tests").join("fixtures").join(name)
}

#[tokio::test]
async fn successful_build_streams_a_size_event_and_finishes_ok() {
    let dir = scratch_dir("success");
    set_fixture(&dir, &repo_fixture("pio-run-success.txt"), None);

    let supervisor = ProcessSupervisor::new();
    let pio = fake_pio();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    spawn_pio_run(&supervisor, &pio, &dir, vec![], "pio-run-build", tx).await.expect("spawn");

    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }

    assert!(matches!(events.first(), Some(ProcEvent::Started { .. })));
    assert!(!events.iter().any(|e| matches!(e, ProcEvent::Defect { .. })), "a clean build must report no defects");

    let size = events.iter().find_map(|e| match e {
        ProcEvent::Size { usage, .. } => Some(usage),
        _ => None,
    });
    let size = size.expect("expected a Size event from the real captured RAM/Flash lines");
    assert_eq!(size.ram_used, 22116);
    assert_eq!(size.flash_used, 274536);

    let stages: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ProcEvent::Stage { stage, .. } => Some(stage.clone()),
            _ => None,
        })
        .collect();
    assert!(stages.iter().any(|s| s.starts_with("Compiling ")));
    assert!(stages.iter().any(|s| s.starts_with("Linking ")));

    match events.last() {
        Some(ProcEvent::Finished { success, exit_code, .. }) => {
            assert!(success);
            assert_eq!(*exit_code, 0);
        }
        other => panic!("expected Finished last, got {other:?}"),
    }
}

#[tokio::test]
async fn failed_build_streams_the_two_real_captured_defects_and_finishes_failed() {
    let dir = scratch_dir("fail");
    set_fixture(&dir, &repo_fixture("pio-run-fail.txt"), Some(1));

    let supervisor = ProcessSupervisor::new();
    let pio = fake_pio();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    spawn_pio_run(&supervisor, &pio, &dir, vec![], "pio-run-build", tx).await.expect("spawn");

    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }

    let defects: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ProcEvent::Defect { defect, .. } => Some(defect),
            _ => None,
        })
        .collect();
    assert_eq!(defects.len(), 2, "{defects:?}");
    assert_eq!(defects[0].line, 5);
    assert_eq!(defects[1].line, 9);

    match events.last() {
        Some(ProcEvent::Finished { success, exit_code, .. }) => {
            assert!(!success);
            assert_eq!(*exit_code, 1);
        }
        other => panic!("expected Finished last, got {other:?}"),
    }
}

#[tokio::test]
async fn cancelling_mid_build_kills_the_process_and_reports_an_unsuccessful_finish() {
    let dir = scratch_dir("cancel");
    let mut content = String::new();
    for i in 0..3000 {
        content.push_str(&format!("Compiling .pio/build/esp32dev/file{i}.cpp.o\n"));
    }
    let fixture_path = dir.join("slow.txt");
    std::fs::write(&fixture_path, content).unwrap();
    std::fs::write(dir.join(".fake-cli-fixture"), format!("{}\n1", fixture_path.display())).unwrap();

    let supervisor = ProcessSupervisor::new();
    let pio = fake_pio();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let proc_id = spawn_pio_run(&supervisor, &pio, &dir, vec![], "pio-run-build", tx).await.expect("spawn");

    let _ = rx.recv().await;
    supervisor.terminate(&proc_id).await.expect("terminate");

    let mut saw_finished = false;
    while let Some(ev) = rx.recv().await {
        if let ProcEvent::Finished { success, .. } = ev {
            assert!(!success);
            saw_finished = true;
        }
    }
    assert!(saw_finished);
    assert!(!supervisor.running().iter().any(|p| p.id == proc_id), "the process must be gone after Stop (FR-BUILD-8)");
}
