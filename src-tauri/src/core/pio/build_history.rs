//! `.vibe/builds.json` — `BuildHistory` (`DATA-MODEL.md` §7, `FR-BUILD-6`).

use crate::core::proc::events::SizeUsage;
use crate::core::project::workspace::vibe_dir;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use ts_rs::TS;

const CURRENT_SCHEMA_VERSION: u32 = 1;
const FILE_NAME: &str = "builds.json";
/// "Capped at the most recent 200 entries" — `DATA-MODEL.md` §7.
const MAX_BUILDS: usize = 200;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BuildKind {
    Build,
    Upload,
    Test,
    Check,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DefectCount {
    pub error: u32,
    pub warning: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuildSize {
    pub ram_used: u64,
    pub ram_total: u64,
    pub flash_used: u64,
    pub flash_total: u64,
}

impl From<&SizeUsage> for BuildSize {
    fn from(u: &SizeUsage) -> Self {
        Self {
            ram_used: u.ram_used,
            ram_total: u.ram_total,
            flash_used: u.flash_used,
            flash_total: u.flash_total,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuildRecord {
    pub id: String,
    /// RFC3339.
    pub at: String,
    pub env: String,
    pub kind: BuildKind,
    pub success: bool,
    pub duration_ms: u64,
    pub snapshot: Option<String>,
    pub size: Option<BuildSize>,
    pub defect_count: DefectCount,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuildHistory {
    pub schema_version: u32,
    pub last_good_snapshot: Option<String>,
    pub builds: Vec<BuildRecord>,
}

impl BuildHistory {
    pub fn empty() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            last_good_snapshot: None,
            builds: Vec::new(),
        }
    }

    /// The most recent build/upload's `size`, for computing the next one's delta
    /// (`FR-BUILD-6`: "the delta against the previous successful build").
    pub fn previous_size(&self, env: &str) -> Option<BuildSize> {
        self.builds
            .iter()
            .rev()
            .find(|b| b.env == env && b.success && matches!(b.kind, BuildKind::Build | BuildKind::Upload))
            .and_then(|b| b.size)
    }
}

fn file_path(workspace: &Path) -> PathBuf {
    vibe_dir(workspace).join(FILE_NAME)
}

pub fn load(workspace: &Path) -> std::io::Result<BuildHistory> {
    match std::fs::read(file_path(workspace)) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_else(|_| BuildHistory::empty())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BuildHistory::empty()),
        Err(e) => Err(e),
    }
}

/// Atomic write: temp file in the same directory, `fsync`, rename (`NFR-R3`).
pub fn save(workspace: &Path, history: &BuildHistory) -> std::io::Result<()> {
    let dir = vibe_dir(workspace);
    std::fs::create_dir_all(&dir)?;
    let path = file_path(workspace);
    let tmp = dir.join(format!("{FILE_NAME}.tmp-{}", std::process::id()));
    let json = serde_json::to_vec_pretty(history)?;
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&json)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Appends `record`, capping at [`MAX_BUILDS`] most recent entries, and — if it's a
/// successful build/upload with a recorded snapshot — updates `lastGoodSnapshot`
/// (`FR-SAFE-6`).
pub fn record_build(history: &mut BuildHistory, record: BuildRecord) {
    if record.success && matches!(record.kind, BuildKind::Build | BuildKind::Upload) {
        if let Some(s) = &record.snapshot {
            history.last_good_snapshot = Some(s.clone());
        }
    }
    history.builds.push(record);
    if history.builds.len() > MAX_BUILDS {
        let excess = history.builds.len() - MAX_BUILDS;
        history.builds.drain(0..excess);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vibe-hw-build-history-test-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample(id: &str, success: bool, snapshot: Option<&str>) -> BuildRecord {
        BuildRecord {
            id: id.into(),
            at: "2026-09-20T09:16:20Z".into(),
            env: "esp32dev".into(),
            kind: BuildKind::Build,
            success,
            duration_ms: 21430,
            snapshot: snapshot.map(String::from),
            size: Some(BuildSize {
                ram_used: 44112,
                ram_total: 327680,
                flash_used: 812345,
                flash_total: 4194304,
            }),
            defect_count: DefectCount::default(),
        }
    }

    #[test]
    fn missing_file_loads_empty() {
        let dir = tempdir("missing");
        assert_eq!(load(&dir).unwrap(), BuildHistory::empty());
    }

    #[test]
    fn round_trips_through_save_and_load() {
        let dir = tempdir("roundtrip");
        let mut history = BuildHistory::empty();
        record_build(&mut history, sample("b-1", true, Some("snap-1")));
        save(&dir, &history).unwrap();
        assert_eq!(load(&dir).unwrap(), history);
    }

    #[test]
    fn record_build_updates_last_good_snapshot_only_on_success() {
        let mut history = BuildHistory::empty();
        record_build(&mut history, sample("b-1", false, Some("snap-fail")));
        assert_eq!(history.last_good_snapshot, None);

        record_build(&mut history, sample("b-2", true, Some("snap-ok")));
        assert_eq!(history.last_good_snapshot.as_deref(), Some("snap-ok"));
    }

    #[test]
    fn record_build_caps_at_200_entries() {
        let mut history = BuildHistory::empty();
        for i in 0..250 {
            record_build(&mut history, sample(&format!("b-{i}"), true, Some("s")));
        }
        assert_eq!(history.builds.len(), 200);
        assert_eq!(history.builds[0].id, "b-50");
        assert_eq!(history.builds[199].id, "b-249");
    }

    #[test]
    fn previous_size_finds_the_most_recent_successful_build_for_the_env() {
        let mut history = BuildHistory::empty();
        record_build(&mut history, sample("b-1", true, Some("s1")));
        let mut second = sample("b-2", true, Some("s2"));
        second.size = Some(BuildSize {
            ram_used: 50000,
            ram_total: 327680,
            flash_used: 900000,
            flash_total: 4194304,
        });
        record_build(&mut history, second);

        let prev = history.previous_size("esp32dev").expect("previous size");
        assert_eq!(prev.ram_used, 50000);
    }

    #[test]
    fn previous_size_ignores_failed_builds_and_other_environments() {
        let mut history = BuildHistory::empty();
        record_build(&mut history, sample("b-1", true, Some("s1")));
        let mut failed = sample("b-2", false, None);
        failed.env = "esp32dev".into();
        record_build(&mut history, failed);
        let mut other_env = sample("b-3", true, Some("s3"));
        other_env.env = "uno".into();
        record_build(&mut history, other_env);

        let prev = history.previous_size("esp32dev").expect("previous size");
        assert_eq!(prev.flash_used, 812345); // from b-1, not the failed b-2 or other-env b-3
    }
}
