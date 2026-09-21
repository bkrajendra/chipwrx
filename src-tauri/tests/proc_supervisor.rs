//! M0 acceptance test (`ROADMAP.md`): spawn `FakeCli`, stream 10 000 lines into a channel,
//! cancel mid-stream, and assert every child process is gone.
//!
//! Lives under `tests/` (an integration test) rather than as a unit test inside
//! `core/proc` because `CARGO_BIN_EXE_fake_cli` — the compiled path to the `fake_cli`
//! binary target — is only set by Cargo for integration tests and benches, not for the
//! lib target's own unit tests.

use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use vibe_hardware_lib::core::proc::{ProcId, ProcKind, ProcessSupervisor, SpawnSpec, StdStream};
use vibe_hardware_lib::error::AppError;

fn fake_cli_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake_cli"))
}

/// Writes `n` numbered lines to a fresh temp file and returns its path. Not using the
/// `tempfile` crate here to avoid a dependency for one throwaway test fixture.
fn write_temp_fixture(name: &str, n: usize) -> PathBuf {
    let path = std::env::temp_dir().join(format!("vibe-hw-test-{name}-{}.txt", ProcId::new().0));
    let mut content = String::with_capacity(n * 8);
    for i in 0..n {
        content.push_str(&format!("line {i}\n"));
    }
    std::fs::write(&path, content).expect("write temp fixture");
    path
}

async fn drain_until_gone(supervisor: &ProcessSupervisor, id: &ProcId, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !supervisor.running().iter().any(|p| &p.id == id) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn streams_10000_lines_and_cancel_mid_stream_kills_the_process() {
    let fixture = write_temp_fixture("stream-10k", 10_000);
    let supervisor = ProcessSupervisor::new();
    let (tx, mut rx) = mpsc::unbounded_channel();

    let spec = SpawnSpec {
        program: fake_cli_path(),
        args: vec![
            "--fixture".into(),
            fixture.display().to_string(),
            "--interval-ms".into(),
            "1".into(),
        ],
        cwd: std::env::temp_dir(),
        env: vec![],
        kind: ProcKind::Tool,
        label: "fake-cli-stream-test".into(),
    };

    let handle = supervisor
        .spawn_streaming(spec, tx)
        .await
        .expect("spawn fake_cli");
    assert!(supervisor.running().iter().any(|p| p.id == handle.id));

    // Consume some, but not all, of the 10 000 lines, then cancel mid-stream.
    let mut received = 0u32;
    while let Some(_line) = rx.recv().await {
        received += 1;
        if received == 50 {
            break;
        }
    }
    assert!(
        received < 10_000,
        "test is meaningless if the process finished before we cancelled it"
    );

    supervisor
        .terminate(&handle.id)
        .await
        .expect("terminate mid-stream");

    let gone = drain_until_gone(&supervisor, &handle.id, Duration::from_secs(10)).await;
    assert!(gone, "process was still tracked as running after terminate()");

    let _ = std::fs::remove_file(&fixture);
}

#[tokio::test]
async fn spawn_streaming_runs_to_completion_with_correct_exit_and_line_count() {
    let fixture = write_temp_fixture("small", 25);
    let supervisor = ProcessSupervisor::new();
    let (tx, mut rx) = mpsc::unbounded_channel();

    let spec = SpawnSpec {
        program: fake_cli_path(),
        args: vec!["--fixture".into(), fixture.display().to_string()],
        cwd: std::env::temp_dir(),
        env: vec![],
        kind: ProcKind::Tool,
        label: "fake-cli-completion-test".into(),
    };

    let handle = supervisor
        .spawn_streaming(spec, tx)
        .await
        .expect("spawn fake_cli");

    let mut lines = Vec::new();
    while let Some(line) = rx.recv().await {
        lines.push(line);
    }
    assert_eq!(lines.len(), 25);
    assert_eq!(lines[0].text, "line 0");
    assert!(lines.iter().all(|l| matches!(l.stream, StdStream::Stdout)));

    let gone = drain_until_gone(&supervisor, &handle.id, Duration::from_secs(5)).await;
    assert!(gone, "supervisor did not reap the finished process");

    let _ = std::fs::remove_file(&fixture);
}

#[tokio::test]
async fn stderr_marker_lines_are_routed_to_stderr() {
    let path = std::env::temp_dir().join(format!("vibe-hw-test-stderr-{}.txt", ProcId::new().0));
    std::fs::write(&path, "on stdout\n!ERR!on stderr\n").expect("write fixture");

    let supervisor = ProcessSupervisor::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let spec = SpawnSpec {
        program: fake_cli_path(),
        args: vec!["--fixture".into(), path.display().to_string()],
        cwd: std::env::temp_dir(),
        env: vec![],
        kind: ProcKind::Tool,
        label: "fake-cli-stderr-test".into(),
    };
    supervisor
        .spawn_streaming(spec, tx)
        .await
        .expect("spawn fake_cli");

    let mut lines = Vec::new();
    while let Some(line) = rx.recv().await {
        lines.push(line);
    }
    assert_eq!(lines.len(), 2);
    // stdout and stderr are two independent pipes read by two independent tasks, so their
    // arrival order relative to each other isn't guaranteed — only content per stream is.
    let stdout_lines: Vec<&str> = lines
        .iter()
        .filter(|l| l.stream == StdStream::Stdout)
        .map(|l| l.text.as_str())
        .collect();
    let stderr_lines: Vec<&str> = lines
        .iter()
        .filter(|l| l.stream == StdStream::Stderr)
        .map(|l| l.text.as_str())
        .collect();
    assert_eq!(stdout_lines, vec!["on stdout"]);
    assert_eq!(stderr_lines, vec!["on stderr"]);

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn spawn_capture_returns_exit_code_and_output() {
    let path = std::env::temp_dir().join(format!("vibe-hw-test-capture-{}.txt", ProcId::new().0));
    std::fs::write(&path, "captured line\n").expect("write fixture");

    let supervisor = ProcessSupervisor::new();
    let spec = SpawnSpec {
        program: fake_cli_path(),
        args: vec![
            "--fixture".into(),
            path.display().to_string(),
            "--exit-code".into(),
            "3".into(),
        ],
        cwd: std::env::temp_dir(),
        env: vec![],
        kind: ProcKind::Tool,
        label: "fake-cli-capture-test".into(),
    };

    let output = supervisor.spawn_capture(spec).await.expect("spawn_capture");
    assert_eq!(output.exit_code, 3);
    assert_eq!(output.stdout, "captured line\n");

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn unknown_proc_id_returns_io_error() {
    let supervisor = ProcessSupervisor::new();
    let result = supervisor.terminate(&ProcId::new()).await;
    assert!(matches!(result, Err(AppError::Io { .. })));
}
