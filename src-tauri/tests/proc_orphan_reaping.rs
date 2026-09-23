//! `M9`/`NFR-R2` acceptance test: "killing the app with `SIGKILL` mid-build leaves no
//! orphaned processes on next launch." A real `SIGKILL` of the whole test process can't be
//! simulated from within the test process itself, so this exercises the actual recovery
//! path directly: a real child process, tracked in an on-disk registry exactly the way
//! `ProcessSupervisor` would have left it if this test process had been killed before
//! reaching `kill_all()`, reaped by `reap_orphans_from_previous_run` as if this were the
//! *next* launch.

use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use vibe_hardware_lib::core::proc::{reap_orphans_from_previous_run, ProcKind, ProcessSupervisor, RegisteredProc, SpawnSpec};

fn fake_cli_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake_cli"))
}

fn write_temp_fixture(name: &str, n: usize) -> PathBuf {
    let path = std::env::temp_dir().join(format!("vibe-hw-orphan-test-{name}-{}.txt", uuid::Uuid::new_v4()));
    let mut content = String::with_capacity(n * 8);
    for i in 0..n {
        content.push_str(&format!("line {i}\n"));
    }
    std::fs::write(&path, content).expect("write temp fixture");
    path
}

fn registry_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("vibe-hw-orphan-registry-{name}-{}.json", uuid::Uuid::new_v4()))
}

async fn wait_until_gone(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        #[cfg(unix)]
        let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
        #[cfg(windows)]
        let alive = {
            use windows_sys::Win32::Foundation::CloseHandle;
            use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
            unsafe {
                let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                if h.is_null() {
                    false
                } else {
                    CloseHandle(h);
                    true
                }
            }
        };
        if !alive {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn reaps_a_process_left_behind_by_a_previous_run_that_never_reached_kill_all() {
    // A real long-lived child (10s at 50ms/line), spawned directly — not through a
    // `ProcessSupervisor` at all, to mirror the scenario exactly: on next launch, all we
    // have is a raw pid from a file, no live `Handle`/`Killer` for it.
    let fixture = write_temp_fixture("victim", 200);
    let supervisor = ProcessSupervisor::new();
    let (tx, mut _rx) = mpsc::unbounded_channel();
    let spec = SpawnSpec {
        program: fake_cli_path(),
        args: vec!["--fixture".into(), fixture.display().to_string(), "--interval-ms".into(), "50".into()],
        cwd: std::env::temp_dir(),
        env: vec![],
        kind: ProcKind::Tool,
        label: "orphan-victim".into(),
    };
    let handle = supervisor.spawn_streaming(spec, tx).await.expect("spawn victim");
    let pid = supervisor
        .running()
        .into_iter()
        .find(|p| p.id == handle.id)
        .and_then(|p| p.pid)
        .expect("victim has a pid");

    // Simulate what a previous run's registry would contain right before it was `SIGKILL`ed
    // mid-build: one entry, for this still-running process.
    let reg_path = registry_path("victim");
    let entries = vec![RegisteredProc { pid, label: "orphan-victim".into() }];
    std::fs::write(&reg_path, serde_json::to_vec(&entries).unwrap()).expect("write registry");

    let reaped = reap_orphans_from_previous_run(&reg_path);
    assert_eq!(reaped, vec!["orphan-victim".to_string()]);

    let gone = wait_until_gone(pid, Duration::from_secs(5)).await;
    assert!(gone, "orphaned process was still alive after reap_orphans_from_previous_run");

    // The registry is cleared for this (the "new") session.
    let cleared: Vec<RegisteredProc> = serde_json::from_slice(&std::fs::read(&reg_path).unwrap()).unwrap();
    assert!(cleared.is_empty());

    let _ = std::fs::remove_file(&fixture);
    let _ = std::fs::remove_file(&reg_path);
}

#[tokio::test]
async fn an_already_dead_pid_is_reported_as_not_reaped_and_does_not_error() {
    // A pid that's already gone must not be treated as a hit — this stands in for "the
    // previous run's registry is stale/already cleaned up," which must be a silent no-op.
    let reg_path = registry_path("dead");
    // A pid extremely unlikely to be alive right now, and if it is, it isn't ours — either
    // way `kill_if_alive` returning `false` for a dead one is what's under test; a false
    // positive here would only make the assertion below fail, never panic or corrupt state.
    let entries = vec![RegisteredProc { pid: 1, label: "definitely-not-ours".into() }];
    std::fs::write(&reg_path, serde_json::to_vec(&entries).unwrap()).expect("write registry");

    let reaped = reap_orphans_from_previous_run(&reg_path);
    assert!(reaped.is_empty(), "pid 1 should never be something this app can/should terminate: {reaped:?}");

    let _ = std::fs::remove_file(&reg_path);
}

#[tokio::test]
async fn missing_registry_file_reaps_nothing_and_does_not_error() {
    let reg_path = registry_path("missing");
    let reaped = reap_orphans_from_previous_run(&reg_path);
    assert!(reaped.is_empty());
}
