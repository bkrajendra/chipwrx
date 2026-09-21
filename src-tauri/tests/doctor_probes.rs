//! Integration tests for the Doctor probes (`ROADMAP.md` M1), driven entirely by `FakeCli`
//! standing in for `claude`/`pio`/`python` — no network, hardware, or paid API call.
//!
//! Each test gets its own scratch directory, used both as the probe's `cwd` (where
//! `fake_cli` looks for its `.fake-cli-fixture` marker file) and to hold the fixture
//! itself — see `src-tauri/src/bin/fake_cli.rs`. This keeps tests safe to run in parallel:
//! nothing here mutates process-wide state like an env var.

use std::path::{Path, PathBuf};
use vibe_hardware_lib::core::proc::ProcessSupervisor;
use vibe_hardware_lib::core::toolchain::probes;
use vibe_hardware_lib::core::toolchain::resolve::{Resolution, Source};
use vibe_hardware_lib::core::toolchain::types::ProbeResult;

fn fake_cli_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake_cli"))
}

fn fake_resolution() -> Resolution {
    Resolution {
        program: fake_cli_path(),
        extra_args: Vec::new(),
        source: Source::Path,
    }
}

/// A fresh scratch directory for one test, used as the probe's `cwd`.
fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "vibe-hw-doctor-test-{name}-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Writes `content` as the fixture FakeCli will replay when spawned with `dir` as its cwd.
fn set_fixture(dir: &Path, content: &str) {
    let fixture_path = dir.join("fixture.txt");
    std::fs::write(&fixture_path, content).expect("write fixture");
    std::fs::write(dir.join(".fake-cli-fixture"), fixture_path.display().to_string())
        .expect("write marker file");
}

// ---------------------------------------------------------------------------------------
// claudeBinary
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn claude_binary_missing_when_unresolved() {
    let dir = scratch_dir("claude-missing");
    let supervisor = ProcessSupervisor::new();
    let result = probes::probe_claude_binary(&supervisor, None, &dir).await;
    assert!(matches!(
        result,
        ProbeResult::Missing { install_available: true }
    ));
}

#[tokio::test]
async fn claude_binary_ok_when_version_meets_floor() {
    let dir = scratch_dir("claude-ok");
    set_fixture(&dir, "2.1.211 (Claude Code)\n");
    let supervisor = ProcessSupervisor::new();
    let result = probes::probe_claude_binary(&supervisor, Some(&fake_resolution()), &dir).await;
    match result {
        ProbeResult::Ok { version, .. } => assert_eq!(version, "2.1.211"),
        other => panic!("expected Ok, got {other:?}"),
    }
}

#[tokio::test]
async fn claude_binary_degraded_when_below_floor() {
    let dir = scratch_dir("claude-old");
    set_fixture(&dir, "1.9.0 (Claude Code)\n");
    let supervisor = ProcessSupervisor::new();
    let result = probes::probe_claude_binary(&supervisor, Some(&fake_resolution()), &dir).await;
    match result {
        ProbeResult::Degraded { reason, remediation } => {
            assert!(reason.contains("1.9.0"));
            assert!(remediation.is_some());
        }
        other => panic!("expected Degraded, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------
// claudeAuth (+ capabilities)
// ---------------------------------------------------------------------------------------

const INIT_LINE: &str = r#"{"type":"system","subtype":"init","capabilities":["permission-prompts-none"]}"#;

#[tokio::test]
async fn claude_auth_ok_on_successful_result() {
    let dir = scratch_dir("auth-ok");
    let transcript = format!(
        "{INIT_LINE}\n{}\n",
        r#"{"type":"result","is_error":false,"result":"ok"}"#
    );
    set_fixture(&dir, &transcript);
    let supervisor = ProcessSupervisor::new();
    let outcome = probes::probe_claude_auth(&supervisor, &fake_resolution(), &dir).await;
    assert!(matches!(outcome.result, ProbeResult::Ok { .. }));
    assert_eq!(outcome.capabilities, vec!["permission-prompts-none".to_string()]);
}

#[tokio::test]
async fn claude_auth_degraded_when_authentication_failed() {
    let dir = scratch_dir("auth-failed");
    let transcript = format!(
        "{INIT_LINE}\n{}\n{}\n",
        r#"{"type":"system","subtype":"api_retry","error":"authentication_failed"}"#,
        r#"{"type":"result","is_error":true,"result":"not authenticated"}"#
    );
    set_fixture(&dir, &transcript);
    let supervisor = ProcessSupervisor::new();
    let outcome = probes::probe_claude_auth(&supervisor, &fake_resolution(), &dir).await;
    match outcome.result {
        ProbeResult::Degraded { reason, remediation } => {
            assert!(reason.to_lowercase().contains("signed in"));
            assert!(remediation.is_some());
        }
        other => panic!("expected Degraded, got {other:?}"),
    }
}

#[tokio::test]
async fn claude_auth_degraded_for_billing_error() {
    let dir = scratch_dir("auth-billing");
    let transcript = format!(
        "{}\n{}\n",
        r#"{"type":"system","subtype":"api_retry","error":"billing_error"}"#,
        r#"{"type":"result","is_error":true,"result":"billing problem"}"#
    );
    set_fixture(&dir, &transcript);
    let supervisor = ProcessSupervisor::new();
    let outcome = probes::probe_claude_auth(&supervisor, &fake_resolution(), &dir).await;
    match outcome.result {
        ProbeResult::Degraded { reason, .. } => assert!(reason.to_lowercase().contains("billing")),
        other => panic!("expected Degraded, got {other:?}"),
    }
}

#[tokio::test]
async fn claude_auth_error_when_no_result_line_at_all() {
    let dir = scratch_dir("auth-garbage");
    set_fixture(&dir, "not json at all\n");
    let supervisor = ProcessSupervisor::new();
    let outcome = probes::probe_claude_auth(&supervisor, &fake_resolution(), &dir).await;
    assert!(matches!(outcome.result, ProbeResult::Error { .. }));
}

// ---------------------------------------------------------------------------------------
// pioBinary
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn pio_binary_ok_when_version_meets_floor() {
    let dir = scratch_dir("pio-ok");
    set_fixture(&dir, "PlatformIO Core, version 6.2.0\n");
    let supervisor = ProcessSupervisor::new();
    let result = probes::probe_pio_binary(&supervisor, Some(&fake_resolution()), &dir).await;
    match result {
        ProbeResult::Ok { version, .. } => assert_eq!(version, "6.2.0"),
        other => panic!("expected Ok, got {other:?}"),
    }
}

#[tokio::test]
async fn pio_binary_degraded_when_below_floor() {
    let dir = scratch_dir("pio-old");
    set_fixture(&dir, "PlatformIO Core, version 5.9.0\n");
    let supervisor = ProcessSupervisor::new();
    let result = probes::probe_pio_binary(&supervisor, Some(&fake_resolution()), &dir).await;
    assert!(matches!(result, ProbeResult::Degraded { .. }));
}

// ---------------------------------------------------------------------------------------
// pioCoreDir
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn pio_core_dir_ok_when_dir_exists_and_is_writable() {
    let dir = scratch_dir("core-dir-ok");
    let core_dir = scratch_dir("core-dir-ok-target");
    let json = format!(
        r#"{{"core_version":{{"title":"x","value":"6.2.0"}},"core_dir":{{"title":"x","value":"{}"}},"python_exe":{{"title":"x","value":"/usr/bin/python3"}},"dev_platform_nums":{{"title":"x","value":0}}}}"#,
        core_dir.display().to_string().replace('\\', "\\\\")
    );
    set_fixture(&dir, &format!("{json}\n"));

    let supervisor = ProcessSupervisor::new();
    let outcome = probes::probe_pio_core_dir(&supervisor, Some(&fake_resolution()), &dir).await;

    match outcome.result {
        ProbeResult::Ok { detail, .. } => {
            assert!(detail.unwrap_or_default().contains("no platforms"))
        }
        other => panic!("expected Ok, got {other:?}"),
    }
    let info = outcome.info.expect("info");
    assert_eq!(info.core_dir.as_deref(), Some(core_dir.as_path()));
}

#[tokio::test]
async fn pio_core_dir_degraded_when_dir_missing() {
    let dir = scratch_dir("core-dir-missing");
    let missing = std::env::temp_dir().join(format!("vibe-hw-missing-{}", uuid::Uuid::new_v4()));
    let json = format!(
        r#"{{"core_dir":{{"title":"x","value":"{}"}}}}"#,
        missing.display().to_string().replace('\\', "\\\\")
    );
    set_fixture(&dir, &format!("{json}\n"));

    let supervisor = ProcessSupervisor::new();
    let outcome = probes::probe_pio_core_dir(&supervisor, Some(&fake_resolution()), &dir).await;
    assert!(matches!(outcome.result, ProbeResult::Degraded { .. }));
}

#[tokio::test]
async fn pio_core_dir_missing_when_pio_unresolved() {
    let dir = scratch_dir("core-dir-unresolved");
    let supervisor = ProcessSupervisor::new();
    let outcome = probes::probe_pio_core_dir(&supervisor, None, &dir).await;
    assert!(matches!(outcome.result, ProbeResult::Missing { .. }));
    assert!(outcome.info.is_none());
}

// ---------------------------------------------------------------------------------------
// python
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn python_ok_when_version_meets_floor() {
    let dir = scratch_dir("python-ok");
    set_fixture(&dir, "Python 3.11.4\n");
    let supervisor = ProcessSupervisor::new();
    let result = probes::probe_python(&supervisor, Some(&fake_resolution()), &dir).await;
    match result {
        ProbeResult::Ok { version, .. } => assert_eq!(version, "3.11.4"),
        other => panic!("expected Ok, got {other:?}"),
    }
}

#[tokio::test]
async fn python_missing_never_offers_install() {
    let dir = scratch_dir("python-missing");
    let supervisor = ProcessSupervisor::new();
    let result = probes::probe_python(&supervisor, None, &dir).await;
    assert!(matches!(
        result,
        ProbeResult::Missing { install_available: false }
    ));
}

// ---------------------------------------------------------------------------------------
// git
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn git_probe_runs_against_whatever_git_is_actually_on_this_machine() {
    // Deliberately not using FakeCli here: git's presence/absence is exactly what this
    // probe answers, and every dev/CI machine already has a real, harmless `git --version`.
    let dir = scratch_dir("git");
    let supervisor = ProcessSupervisor::new();
    let result = probes::probe_git(&supervisor, &dir).await;
    // Either shape is a legitimate outcome depending on the host; assert it's one of them
    // rather than assuming git is installed everywhere this suite runs.
    assert!(matches!(result, ProbeResult::Ok { .. } | ProbeResult::Degraded { .. }));
}

// ---------------------------------------------------------------------------------------
// serialPermissions — always Ok off Linux; skip=true short-circuits everywhere.
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn serial_permissions_ok_when_skipped() {
    let dir = scratch_dir("serial-skip");
    let supervisor = ProcessSupervisor::new();
    let result = probes::probe_serial_permissions(&supervisor, None, true, &dir).await;
    assert!(matches!(result, ProbeResult::Ok { .. }));
}

#[cfg(not(target_os = "linux"))]
#[tokio::test]
async fn serial_permissions_always_ok_off_linux() {
    let dir = scratch_dir("serial-nonlinux");
    let supervisor = ProcessSupervisor::new();
    let result = probes::probe_serial_permissions(&supervisor, None, false, &dir).await;
    assert!(matches!(result, ProbeResult::Ok { .. }));
}
