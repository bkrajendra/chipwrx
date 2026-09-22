//! The single error enum surfaced to the frontend. See `ARCHITECTURE.md` §6.
//!
//! Every variant carries a stable `code` (the serde tag) and enough structured data for
//! the UI to render its own wording and remediation — there is no generic "Something went
//! wrong" dialog anywhere in this product.

use serde::Serialize;
use ts_rs::TS;

/// See `SPEC.md` §8 open question 5: this shape is not specified anywhere else in the
/// pack and mirrors `ChatEvent::PermissionDenied`'s payload.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PermissionDenial {
    pub tool: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "code",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase"
)]
pub enum AppError {
    ToolMissing {
        tool: String,
        install_action: bool,
    },
    ToolTooOld {
        tool: String,
        found: String,
        minimum: String,
    },
    ClaudeUnauthenticated,
    ClaudePermissionDenied {
        denials: Vec<PermissionDenial>,
    },
    ClaudeInterrupted {
        session_id: String,
    },
    NetworkUnavailable {
        host: String,
    },
    PioCommandFailed {
        argv: Vec<String>,
        exit_code: i32,
        tail: String,
    },
    /// See `SPEC.md` §8 open question 8: a claude-specific analogue of `PioCommandFailed`
    /// for a turn whose process exited non-zero (or zero with no parseable `result` line)
    /// without a clean interruption — `ClaudeInterrupted` covers the SIGTERM/exit-143 case.
    ClaudeProcessFailed {
        exit_code: i32,
        tail: String,
    },
    BuildFailed {
        defects: usize,
    },
    PortBusy {
        port: String,
        held_by: String,
    },
    PortDisappeared {
        port: String,
    },
    NotAPioProject {
        path: String,
    },
    IniParse {
        path: String,
        line: Option<u32>,
        message: String,
    },
    IniChangedOnDisk {
        path: String,
    },
    WorkspaceUntrusted {
        hooks: Vec<String>,
        mcp_servers: Vec<String>,
    },
    /// See `SPEC.md` §8 open question 12: the safety-net snapshot/revert engine
    /// (`core::snapshot`) has no dedicated variant in `ARCHITECTURE.md` §6's original
    /// list. `message` is `git2::Error::message()` or an equivalent local diagnostic —
    /// never surfaced with a git plumbing command that could resemble a shell string.
    SnapshotFailed {
        message: String,
    },
    /// `FR-BUILD-3` Safe policy: Upload attempted without (or after invalidating) a
    /// successful current-state Build. See `SPEC.md` §8 open question 19.
    UploadBlocked {
        reason: String,
    },
    Io {
        message: String,
    },
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Debug is adequate here: this is for logs, never shown to the user directly.
        // The UI renders its own wording per `code` via the error-renderer registry.
        write!(f, "{self:?}")
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Io {
            message: err.to_string(),
        }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::Io {
            message: err.to_string(),
        }
    }
}

impl From<git2::Error> for AppError {
    fn from(err: git2::Error) -> Self {
        AppError::SnapshotFailed {
            message: err.message().to_string(),
        }
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
