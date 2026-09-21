//! Snapshot/changes IPC types. `IPC-CONTRACT.md` §8. Settled in `core::claude::types`
//! during M3 (ahead of this module existing, exactly like `core::proc::events::{Defect,
//! SizeUsage}` were settled ahead of `core/diag`) and moved here now that `core/snapshot`
//! is real.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct SnapshotId(pub String);

impl std::fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub status: ChangeStatus,
    pub additions: u32,
    pub deletions: u32,
    /// Drives the `FR-SAFE-4` warning banner: `true` when `path` isn't under `src/`,
    /// `include/`, `lib/`, `test/`, `data/`, or exactly `platformio.ini`.
    pub outside_expected_dirs: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
}
