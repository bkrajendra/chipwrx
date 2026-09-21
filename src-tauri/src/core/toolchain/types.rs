//! Doctor & toolchain IPC types. See `IPC-CONTRACT.md` §2.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "status")]
pub enum ProbeResult {
    Ok {
        version: String,
        path: Option<String>,
        detail: Option<String>,
    },
    Missing {
        install_available: bool,
    },
    Degraded {
        reason: String,
        remediation: Option<Remediation>,
    },
    Error {
        detail: String,
    },
    Probing,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RemediationKind {
    InstallClaude,
    InstallPio,
    AuthenticateClaude,
    InstallUdevRules,
    OpenUrl,
    ShowCommand,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Remediation {
    pub kind: RemediationKind,
    pub label: String,
    pub command_preview: Option<Vec<String>>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub claude_binary: ProbeResult,
    pub claude_auth: ProbeResult,
    pub claude_capabilities: Vec<String>,
    pub pio_binary: ProbeResult,
    pub pio_core_dir: ProbeResult,
    pub python: ProbeResult,
    pub network_registry: ProbeResult,
    pub serial_permissions: ProbeResult,
    pub git: ProbeResult,
    /// RFC3339.
    pub probed_at: String,
}
