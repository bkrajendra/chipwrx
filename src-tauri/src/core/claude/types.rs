//! Claude turn IPC types. See `IPC-CONTRACT.md` §4 (and §8 for `SnapshotId`/`FileChange`,
//! which `ChatEvent::ChangesComputed` references — defined here, not populated until
//! `core/snapshot` exists in M4, exactly like `core::proc::events::{Defect, SizeUsage}`
//! were settled in M2 ahead of `core/diag` in M5).

use super::ids::{SessionId, TurnId};
use crate::core::settings::PermissionPolicySetting;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// `IPC-CONTRACT.md` §4 deliberately gives `TurnRequest.policy` its own type rather than
/// reusing `core::settings::PermissionPolicySetting` — it's a per-turn override, not the
/// persisted setting. Structurally identical; kept separate to match the contract.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PermissionPolicy {
    Guarded,
    Assisted,
    Unrestricted,
}

impl From<PermissionPolicy> for PermissionPolicySetting {
    fn from(p: PermissionPolicy) -> Self {
        match p {
            PermissionPolicy::Guarded => PermissionPolicySetting::Guarded,
            PermissionPolicy::Assisted => PermissionPolicySetting::Assisted,
            PermissionPolicy::Unrestricted => PermissionPolicySetting::Unrestricted,
        }
    }
}

impl From<PermissionPolicySetting> for PermissionPolicy {
    fn from(p: PermissionPolicySetting) -> Self {
        match p {
            PermissionPolicySetting::Guarded => PermissionPolicy::Guarded,
            PermissionPolicySetting::Assisted => PermissionPolicy::Assisted,
            PermissionPolicySetting::Unrestricted => PermissionPolicy::Unrestricted,
        }
    }
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnRequest {
    pub workspace: String,
    pub prompt: String,
    /// Relative paths under `.vibe/attachments` (`FR-CHAT-9`, M8). Always empty in M3 —
    /// no attachment UI yet.
    pub attachments: Vec<String>,
    pub policy: Option<PermissionPolicy>,
    pub model: Option<String>,
}

// ---------------------------------------------------------------------------------------
// §8 — Snapshots and changes. Referenced by `ChatEvent::ChangesComputed` but not populated
// until `core/snapshot` exists (M4). Settled now so every consumer compiles against the
// same shape from the start.
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct SnapshotId(pub String);

#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub status: ChangeStatus,
    pub additions: u32,
    pub deletions: u32,
    pub outside_expected_dirs: bool,
}

// ---------------------------------------------------------------------------------------
// §4 — ChatEvent
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type",
    content = "data"
)]
pub enum ChatEvent {
    /// Emitted once we have parsed `system/init`.
    SessionReady {
        session_id: SessionId,
        model: String,
        tools: Vec<String>,
        capabilities: Vec<String>,
        mcp_errors: Vec<String>,
        plugin_errors: Vec<String>,
    },

    /// Coalesced assistant prose. `blockIndex` groups deltas into content blocks. `text` is
    /// the chunk accumulated since the last flush, not the running total — the frontend
    /// appends it (`ARCHITECTURE.md` §8: coalesced, not resent-in-full).
    TextDelta {
        turn_id: TurnId,
        block_index: u32,
        text: String,
    },

    /// A complete assistant content block arrived.
    TextBlock {
        turn_id: TurnId,
        block_index: u32,
        text: String,
    },

    /// Thinking blocks, when present, rendered collapsed by default. Delta-only — there is
    /// no `ThinkingBlock` structural event; the deltas are the whole story.
    ThinkingDelta {
        turn_id: TurnId,
        block_index: u32,
        text: String,
    },

    /// A tool call began. `input` may be partial while streaming.
    ToolCallStarted {
        turn_id: TurnId,
        tool_use_id: String,
        name: String,
        input_preview: String,
    },
    ToolCallInputDelta {
        turn_id: TurnId,
        tool_use_id: String,
        partial_json: String,
    },
    ToolCallCompleted {
        turn_id: TurnId,
        tool_use_id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        turn_id: TurnId,
        tool_use_id: String,
        is_error: bool,
        summary: String,
        full: Option<String>,
    },

    /// Subagent attribution: non-null `parent_tool_use_id` on `assistant`/`user` messages.
    SubagentMessage {
        turn_id: TurnId,
        parent_tool_use_id: String,
        role: String,
        text: String,
    },

    ApiRetry {
        turn_id: TurnId,
        attempt: u32,
        max_retries: u32,
        retry_delay_ms: u64,
        error: String,
        error_status: Option<u16>,
    },

    PermissionDenied {
        turn_id: TurnId,
        tool: String,
        reason: String,
    },

    CompactBoundary {
        turn_id: TurnId,
    },

    /// Final. Always emitted, even on failure or interruption — except when the process
    /// itself never produced one, in which case `Failed` is emitted instead
    /// (`FR-CHAT-11`).
    Result {
        turn_id: TurnId,
        session_id: SessionId,
        subtype: String,
        is_error: bool,
        num_turns: u32,
        duration_ms: u64,
        duration_api_ms: u64,
        total_cost_usd: Option<f64>,
        result_text: Option<String>,
        permission_denials: Vec<String>,
    },

    /// Emitted after `Result`, once the snapshot diff has been computed. Not emitted until
    /// `core/snapshot` exists (M4) — see the module doc comment.
    ChangesComputed {
        turn_id: TurnId,
        snapshot: SnapshotId,
        changes: Vec<FileChange>,
    },

    /// Process-level failure with no usable `Result` event: a non-zero/unexpected exit
    /// (`AppError::ClaudeProcessFailed`) or a clean interruption
    /// (`AppError::ClaudeInterrupted`, `FR-CHAT-5`).
    Failed {
        turn_id: TurnId,
        error: AppError,
    },
}

impl ChatEvent {
    /// `DATA-MODEL.md` §6: "Do not persist every `TextDelta`... store the reconstructed
    /// `assistantText` and the structural events." Filters out the three high-frequency
    /// delta variants; everything else is structural and worth keeping in `TurnRecord`.
    pub fn is_persistable(&self) -> bool {
        !matches!(
            self,
            ChatEvent::TextDelta { .. } | ChatEvent::ThinkingDelta { .. } | ChatEvent::ToolCallInputDelta { .. }
        )
    }
}
