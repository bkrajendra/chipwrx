//! Orchestrates the nine probes into a `DoctorReport`, and a small cache matching
//! `FR-SETUP-2`'s re-run policy (on demand, on window focus after >6h, after any install).

use super::probes::{self, resolve_tool};
use super::resolve::Tool;
use super::types::{DoctorReport, ProbeResult};
use crate::core::proc::ProcessSupervisor;
use crate::core::settings::ToolchainSettings;
use std::path::Path;
use std::time::{Duration, Instant};

pub struct DoctorContext<'a> {
    pub supervisor: &'a ProcessSupervisor,
    pub settings: &'a ToolchainSettings,
    /// An app-owned scratch directory — never the user's workspace. Used as `cwd` for every
    /// spawned probe, including the Claude auth probe turn (see `probes::probe_claude_auth`'s
    /// docs on why that matters).
    pub scratch_dir: &'a Path,
    pub http_client: &'a reqwest::Client,
    /// PlatformIO's own `disable_udev_rules_check` setting (`pio settings get`) — `false`
    /// (always check) until a project/global-settings screen can read the real value.
    pub disable_udev_rules_check: bool,
    /// Always `probes::REGISTRY_PROBE_URL` in production; overridable so tests can point
    /// the network probe at an unreachable local address instead of the real internet.
    pub network_probe_url: &'a str,
}

pub async fn run_doctor(ctx: DoctorContext<'_>) -> DoctorReport {
    let (claude_res, pio_res, python_res) = tokio::join!(
        resolve_tool(Tool::Claude, ctx.settings.claude_path.as_deref(), ctx.supervisor),
        resolve_tool(Tool::Pio, ctx.settings.pio_path.as_deref(), ctx.supervisor),
        resolve_tool(Tool::Python, ctx.settings.python_path.as_deref(), ctx.supervisor),
    );

    let claude_auth_fut = async {
        match &claude_res {
            Some(r) => {
                let outcome = probes::probe_claude_auth(ctx.supervisor, r, ctx.scratch_dir).await;
                (outcome.result, outcome.capabilities)
            }
            None => (
                ProbeResult::Missing {
                    install_available: false,
                },
                Vec::new(),
            ),
        }
    };

    let (
        claude_binary_result,
        pio_binary_result,
        python_result,
        network_result,
        git_result,
        (claude_auth_result, claude_capabilities),
        pio_core_dir_outcome,
    ) = tokio::join!(
        probes::probe_claude_binary(ctx.supervisor, claude_res.as_ref(), ctx.scratch_dir),
        probes::probe_pio_binary(ctx.supervisor, pio_res.as_ref(), ctx.scratch_dir),
        probes::probe_python(ctx.supervisor, python_res.as_ref(), ctx.scratch_dir),
        probes::probe_network_registry(ctx.http_client, ctx.network_probe_url),
        probes::probe_git(ctx.supervisor, ctx.scratch_dir),
        claude_auth_fut,
        probes::probe_pio_core_dir(ctx.supervisor, pio_res.as_ref(), ctx.scratch_dir),
    );

    let python_exe_for_udev = pio_core_dir_outcome.info.as_ref().and_then(|i| i.python_exe.clone());
    let serial_result = probes::probe_serial_permissions(
        ctx.supervisor,
        python_exe_for_udev.as_deref(),
        ctx.disable_udev_rules_check,
        ctx.scratch_dir,
    )
    .await;

    DoctorReport {
        claude_binary: claude_binary_result,
        claude_auth: claude_auth_result,
        claude_capabilities,
        pio_binary: pio_binary_result,
        pio_core_dir: pio_core_dir_outcome.result,
        python: python_result,
        network_registry: network_result,
        serial_permissions: serial_result,
        git: git_result,
        probed_at: chrono::Utc::now().to_rfc3339(),
    }
}

/// `FR-SETUP-2`: re-run on demand, on window focus if the last run is older than 6h, and
/// after any install action. The demand/install cases are just "call `run_doctor` again and
/// `set`"; this only needs to answer the time-based question.
pub struct DoctorCache {
    report: Option<DoctorReport>,
    last_probed: Option<Instant>,
}

impl Default for DoctorCache {
    fn default() -> Self {
        Self::new()
    }
}

impl DoctorCache {
    pub fn new() -> Self {
        Self {
            report: None,
            last_probed: None,
        }
    }

    pub fn get(&self) -> Option<&DoctorReport> {
        self.report.as_ref()
    }

    pub fn set(&mut self, report: DoctorReport) {
        self.last_probed = Some(Instant::now());
        self.report = Some(report);
    }

    pub fn is_stale(&self, max_age: Duration) -> bool {
        match self.last_probed {
            None => true,
            Some(t) => t.elapsed() >= max_age,
        }
    }
}

pub const FOCUS_REPROBE_AGE: Duration = Duration::from_secs(6 * 60 * 60);

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_report() -> DoctorReport {
        DoctorReport {
            claude_binary: ProbeResult::Probing,
            claude_auth: ProbeResult::Probing,
            claude_capabilities: vec![],
            pio_binary: ProbeResult::Probing,
            pio_core_dir: ProbeResult::Probing,
            python: ProbeResult::Probing,
            network_registry: ProbeResult::Probing,
            serial_permissions: ProbeResult::Probing,
            git: ProbeResult::Probing,
            probed_at: "2026-09-21T00:00:00Z".into(),
        }
    }

    #[test]
    fn empty_cache_is_stale() {
        let cache = DoctorCache::new();
        assert!(cache.is_stale(Duration::from_secs(1)));
        assert!(cache.get().is_none());
    }

    #[test]
    fn freshly_set_cache_is_not_stale() {
        let mut cache = DoctorCache::new();
        cache.set(sample_report());
        assert!(!cache.is_stale(Duration::from_secs(3600)));
        assert!(cache.get().is_some());
    }

    #[test]
    fn cache_is_stale_once_max_age_elapses() {
        let mut cache = DoctorCache::new();
        cache.set(sample_report());
        assert!(cache.is_stale(Duration::from_millis(0)));
    }
}
