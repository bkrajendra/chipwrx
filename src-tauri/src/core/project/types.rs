//! Project persistence types. `DATA-MODEL.md` §4 (`.vibe/project.json`) and §5
//! (`projects.json`); the IPC-facing `ProjectEntry` is `IPC-CONTRACT.md` §3.

use crate::core::settings::{PermissionPolicySetting, PipelinePolicySetting};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

// ---------------------------------------------------------------------------------------
// .vibe/project.json — ProjectSettings (per-workspace)
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CreatedBy {
    #[serde(rename = "vibe-hardware")]
    #[ts(rename = "vibe-hardware")]
    VibeHardware,
    Opened,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StickyHwid {
    pub vid: String,
    pub pid: String,
    pub serial: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSettings {
    pub preferred_port: Option<String>,
    pub sticky_hwid: Option<StickyHwid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectClaudeSettings {
    pub session_id: Option<String>,
    pub permission_policy: Option<PermissionPolicySetting>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPipelineSettings {
    pub policy: Option<PipelinePolicySetting>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSettings {
    pub schema_version: u32,
    /// `WorkspaceId`, generated on first open/create and stable across moves — this is
    /// what the registry reconciles on, not the path.
    pub id: String,
    pub name: String,
    pub active_env: Option<String>,
    #[serde(default)]
    pub device: DeviceSettings,
    #[serde(default)]
    pub claude: ProjectClaudeSettings,
    #[serde(default)]
    pub pipeline: ProjectPipelineSettings,
    pub trusted: bool,
    /// RFC3339.
    pub trust_scan_at: Option<String>,
    pub created_by: CreatedBy,
    /// RFC3339.
    pub created_at: String,
}

// ---------------------------------------------------------------------------------------
// projects.json — ProjectRegistry (app-wide)
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRegistryEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub board_id: Option<String>,
    pub active_env: Option<String>,
    /// RFC3339.
    pub last_opened: Option<String>,
    pub last_build_ok: Option<bool>,
    pub trusted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRegistry {
    pub schema_version: u32,
    pub projects: Vec<ProjectRegistryEntry>,
}

/// `IPC-CONTRACT.md` §3 — the registry entry plus `exists`, computed at read time rather
/// than stored (a moved/deleted folder is flagged, never silently dropped — `FR-PROJ-4`).
#[derive(Debug, Clone, Serialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub board_id: Option<String>,
    pub active_env: Option<String>,
    pub last_opened: Option<String>,
    pub last_build_ok: Option<bool>,
    pub exists: bool,
    pub trusted: bool,
}

// ---------------------------------------------------------------------------------------
// Workspace trust (NFR-S5)
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, TS, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrustScan {
    /// Hook event names found in `.claude/settings.json`'s `hooks` object, e.g.
    /// `"SessionStart"`, `"PreToolUse"`.
    pub hooks: Vec<String>,
    /// Server names from `.mcp.json`'s `mcpServers` object.
    pub mcp_servers: Vec<String>,
    /// File stems under `.claude/agents/`.
    pub agents: Vec<String>,
}

impl TrustScan {
    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty() && self.mcp_servers.is_empty() && self.agents.is_empty()
    }
}
