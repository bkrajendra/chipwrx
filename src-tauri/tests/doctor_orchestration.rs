//! End-to-end test of `run_doctor` (`ROADMAP.md` M1 acceptance): all nine probes run
//! concurrently and produce a complete `DoctorReport`, driven entirely by `FakeCli` and a
//! guaranteed-unreachable local address for the network probe — no real internet access.

use std::path::PathBuf;
use vibe_hardware_lib::core::proc::ProcessSupervisor;
use vibe_hardware_lib::core::settings::ToolchainSettings;
use vibe_hardware_lib::core::toolchain::doctor::{run_doctor, DoctorContext};
use vibe_hardware_lib::core::toolchain::types::ProbeResult;

fn fake_cli_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake_cli"))
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "vibe-hw-doctor-e2e-{name}-{}",
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
async fn full_doctor_run_with_everything_healthy() {
    let scratch = scratch_dir("all-green");
    let supervisor = ProcessSupervisor::new();
    let settings = ToolchainSettings {
        claude_path: Some(fake_cli_path().display().to_string()),
        pio_path: Some(fake_cli_path().display().to_string()),
        python_path: Some(fake_cli_path().display().to_string()),
        auto_probe_on_focus: true,
    };
    // `run_doctor` spawns every probe with the same `scratch_dir` as cwd, so claude/pio/
    // python all read the same fixture through FakeCli's `.fake-cli-fixture` marker file —
    // one shared transcript has to satisfy every probe's parsing at once. `99.0.0` clears
    // every version floor (Claude, PlatformIO, and Python each need very different minima);
    // the exact realistic version strings are covered per-probe in `doctor_probes.rs`.
    set_fixture(
        &scratch,
        "99.0.0\n\
         {\"type\":\"system\",\"subtype\":\"init\",\"capabilities\":[\"permission-prompts-none\"]}\n\
         {\"type\":\"result\",\"is_error\":false,\"result\":\"ok\"}\n",
    );

    let client = reqwest::Client::new();
    let ctx = DoctorContext {
        supervisor: &supervisor,
        settings: &settings,
        scratch_dir: &scratch,
        http_client: &client,
        disable_udev_rules_check: true,
        network_probe_url: "http://127.0.0.1:1/unreachable",
    };

    let report = run_doctor(ctx).await;

    assert!(matches!(report.claude_binary, ProbeResult::Ok { .. }));
    assert!(matches!(report.claude_auth, ProbeResult::Ok { .. }));
    assert_eq!(report.claude_capabilities, vec!["permission-prompts-none".to_string()]);
    assert!(matches!(report.pio_binary, ProbeResult::Ok { .. }));
    // pio_core_dir: `pio system info --json-output` gets the same transcript, which isn't
    // valid JSON for that probe, so it degrades to Error — a real `pio` would answer this
    // one differently. See the narrower `pio_core_dir_*` tests in `doctor_probes.rs`.
    assert!(matches!(report.pio_core_dir, ProbeResult::Error { .. }));
    assert!(matches!(report.python, ProbeResult::Ok { .. }));
    assert!(matches!(report.network_registry, ProbeResult::Degraded { .. }));
    assert!(matches!(report.serial_permissions, ProbeResult::Ok { .. }));
    assert!(matches!(report.git, ProbeResult::Ok { .. } | ProbeResult::Degraded { .. }));
    assert!(!report.probed_at.is_empty());
}

#[tokio::test]
async fn full_doctor_run_with_nothing_installed() {
    let scratch = scratch_dir("all-red");
    let supervisor = ProcessSupervisor::new();
    let settings = ToolchainSettings {
        claude_path: Some("/does/not/exist/claude".into()),
        pio_path: Some("/does/not/exist/pio".into()),
        python_path: Some("/does/not/exist/python3".into()),
        auto_probe_on_focus: true,
    };

    let client = reqwest::Client::new();
    let ctx = DoctorContext {
        supervisor: &supervisor,
        settings: &settings,
        scratch_dir: &scratch,
        http_client: &client,
        disable_udev_rules_check: true,
        network_probe_url: "http://127.0.0.1:1/unreachable",
    };

    let report = run_doctor(ctx).await;

    // On CI (Linux), the settings path is bogus but PATH/candidate fallback might still
    // find a real `claude`/`pio`/`python3` on the runner — so assert the shape that's
    // guaranteed regardless of the host: auth mirrors binary resolution, and network is
    // unreachable either way.
    match &report.claude_binary {
        ProbeResult::Missing { .. } => {
            assert!(matches!(report.claude_auth, ProbeResult::Missing { .. }));
            assert!(report.claude_capabilities.is_empty());
        }
        ProbeResult::Ok { .. } => {} // a real claude happened to be on this host's PATH
        other => panic!("unexpected claude_binary shape: {other:?}"),
    }
    assert!(matches!(report.network_registry, ProbeResult::Degraded { .. }));
}
