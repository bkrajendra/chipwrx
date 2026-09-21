//! Integration tests for the install flows (`ROADMAP.md` M1), with `FakeCli` standing in
//! for `curl` and the script runner (`bash`/`python`) — no network access.

use std::path::PathBuf;
use vibe_hardware_lib::core::proc::ProcessSupervisor;
use vibe_hardware_lib::core::toolchain::install::{self, InstallEvent, InstallTools};
use vibe_hardware_lib::core::toolchain::resolve::Platform;

fn fake_cli_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake_cli"))
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "vibe-hw-install-test-{name}-{}",
        uuid::Uuid::new_v4()
    ));
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
async fn claude_install_succeeds_end_to_end_with_fake_tools() {
    let dir = scratch_dir("claude-success");
    set_fixture(&dir, "Claude Code installed at ~/.local/bin/claude\n");

    let supervisor = ProcessSupervisor::new();
    let tools = InstallTools {
        curl: fake_cli_path(),
        script_runner: fake_cli_path(),
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let handle = tokio::spawn(async move {
        install::run_claude_install(&supervisor, Platform::Linux, &tools, &dir, tx).await
    });

    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }
    let result = handle.await.expect("task joined");
    assert!(result.is_ok(), "install failed: {result:?}");

    let started_count = events
        .iter()
        .filter(|e| matches!(e, InstallEvent::Started { .. }))
        .count();
    assert_eq!(started_count, 2, "expected one Started per step (download + run)");

    match events.last() {
        Some(InstallEvent::Finished { success, exit_code }) => {
            assert!(success);
            assert_eq!(*exit_code, 0);
        }
        other => panic!("expected Finished as the last event, got {other:?}"),
    }
}

#[tokio::test]
async fn claude_install_stops_and_reports_failure_when_download_fails() {
    let dir = scratch_dir("claude-download-fail");
    // FakeCli's own marker file drives BOTH the curl step and the run step in this test;
    // a non-zero exit code applies to whichever step runs — since download runs first and
    // this test only cares that a failing download halts the flow before the run step's
    // Started event, that's exactly what we assert below.
    let fixture_path = dir.join("fixture.txt");
    std::fs::write(&fixture_path, "curl: could not resolve host\n").unwrap();
    std::fs::write(
        dir.join(".fake-cli-fixture"),
        format!("{}\n0\n7", fixture_path.display()),
    )
    .unwrap();

    let supervisor = ProcessSupervisor::new();
    let tools = InstallTools {
        curl: fake_cli_path(),
        script_runner: fake_cli_path(),
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let handle = tokio::spawn(async move {
        install::run_claude_install(&supervisor, Platform::Linux, &tools, &dir, tx).await
    });

    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }
    let result = handle.await.expect("task joined");
    assert!(result.is_err());

    let started_count = events
        .iter()
        .filter(|e| matches!(e, InstallEvent::Started { .. }))
        .count();
    assert_eq!(started_count, 1, "the run step must never start after a failed download");
}

#[tokio::test]
async fn pio_install_succeeds_end_to_end_with_fake_tools() {
    let dir = scratch_dir("pio-success");
    set_fixture(&dir, "PlatformIO Core has been successfully installed!\n");

    let supervisor = ProcessSupervisor::new();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let handle = tokio::spawn(async move {
        install::run_pio_install(&supervisor, &fake_cli_path(), fake_cli_path(), vec![], &dir, tx).await
    });

    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }
    let result = handle.await.expect("task joined");
    assert!(result.is_ok(), "install failed: {result:?}");
    assert!(matches!(
        events.last(),
        Some(InstallEvent::Finished { success: true, exit_code: 0 })
    ));
}

#[test]
fn resolve_system_tools_finds_a_real_curl_on_this_machine() {
    // Not FakeCli-based on purpose: this only checks resolution against whatever `curl` is
    // actually on this dev/CI machine's PATH (curl ships with modern macOS/Windows/most
    // Linux distros) — it never invokes it.
    let result = install::resolve_system_tools(vibe_hardware_lib::core::toolchain::resolve::current_platform());
    assert!(result.is_ok(), "expected curl to be found: {result:?}");
}
