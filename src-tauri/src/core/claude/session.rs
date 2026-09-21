//! `.vibe/sessions/<session-id>.jsonl` and `.vibe/sessions/index.json` (`DATA-MODEL.md`
//! §6, `FR-CHAT-6`). The `.jsonl` file is append-only by design — "a crash mid-turn loses
//! at most the current line, never the history" — so it's written with `OpenOptions::
//! append`, not the temp-file-then-rename pattern the rest of this app's stores use;
//! `index.json` is small and rewritten each time, so it *does* use that pattern.

use super::types::ChatEvent;
use crate::core::project::workspace::vibe_dir;
use crate::core::settings::PermissionPolicySetting;
use crate::core::snapshot::types::FileChange;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use ts_rs::TS;

const CURRENT_SCHEMA_VERSION: u32 = 1;
/// `DATA-MODEL.md` §6: "`title` is derived from the first prompt (first ~60 chars)."
const TITLE_CHARS: usize = 60;

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TurnToolCall {
    pub tool_use_id: String,
    pub name: String,
    pub input: serde_json::Value,
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TurnResultMeta {
    pub subtype: String,
    pub is_error: bool,
    pub num_turns: u32,
    pub duration_ms: u64,
    pub duration_api_ms: u64,
    pub total_cost_usd: Option<f64>,
    pub permission_denials: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TurnRecord {
    pub schema_version: u32,
    pub turn_id: String,
    pub session_id: String,
    /// RFC3339.
    pub started_at: String,
    /// RFC3339. `None` if the turn is still recorded as in-flight (shouldn't happen once
    /// `append_turn` runs — every call site finalizes `endedAt` first).
    pub ended_at: Option<String>,
    pub prompt: String,
    pub attachments: Vec<String>,
    pub model: String,
    pub policy: PermissionPolicySetting,
    /// Redacted (`core::redact::redact`) — for the diagnostics bundle, not for replay.
    pub argv: Vec<String>,
    /// The parsed `ChatEvent` stream minus `TextDelta`/`ThinkingDelta`/
    /// `ToolCallInputDelta` noise (`DATA-MODEL.md` §6). Stored as loose JSON rather than
    /// typed `ChatEvent` so this record can round-trip through `Deserialize` without
    /// widening `AppError`'s own derive surface just for archival storage.
    pub events: Vec<serde_json::Value>,
    pub assistant_text: String,
    pub tool_calls: Vec<TurnToolCall>,
    pub result: Option<TurnResultMeta>,
    /// The snapshot taken just before this turn ran (`core::snapshot`, `FR-SAFE-1`).
    /// `#[serde(default)]` so a record a pre-M4 build wrote still loads.
    #[serde(default)]
    pub snapshot_before: Option<String>,
    /// Computed once, right after the turn, from `snapshotBefore` vs the working tree at
    /// that moment (`FR-SAFE-2`) — an audit trail of what the turn did, independent of
    /// `changes_for_turn`'s live recomputation (`SPEC.md` §8 open question 14).
    #[serde(default)]
    pub changes: Vec<FileChange>,
}

/// Builds `TurnRecord.events` from the events a turn actually emitted, filtering per
/// `ChatEvent::is_persistable` and converting leniently (a serialization failure — none
/// expected in practice — just drops that one event rather than failing the whole record).
pub fn persistable_events(events: &[ChatEvent]) -> Vec<serde_json::Value> {
    events
        .iter()
        .filter(|e| e.is_persistable())
        .filter_map(|e| serde_json::to_value(e).ok())
        .collect()
}

fn sessions_dir(workspace: &Path) -> PathBuf {
    vibe_dir(workspace).join("sessions")
}

fn session_path(workspace: &Path, session_id: &str) -> PathBuf {
    sessions_dir(workspace).join(format!("{session_id}.jsonl"))
}

fn index_path(workspace: &Path) -> PathBuf {
    sessions_dir(workspace).join("index.json")
}

/// Appends one line to the session's `.jsonl` transcript, creating the file and its parent
/// directory if needed.
pub fn append_turn(workspace: &Path, record: &TurnRecord) -> std::io::Result<()> {
    let dir = sessions_dir(workspace);
    std::fs::create_dir_all(&dir)?;
    let path = session_path(workspace, &record.session_id);
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    let line = serde_json::to_string(record)?;
    writeln!(f, "{line}")?;
    f.sync_all()?;
    Ok(())
}

/// Loads every complete `TurnRecord` from a session's transcript, oldest first.
/// `DATA-MODEL.md` §12: a truncated trailing line (a crash mid-write) is dropped silently
/// — the interrupted turn is recoverable via `--resume`, not from this file. Missing file
/// = no turns yet, not an error.
pub fn load_turns(workspace: &Path, session_id: &str) -> std::io::Result<Vec<TurnRecord>> {
    let path = session_path(workspace, session_id);
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let lines: Vec<&str> = contents.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut out = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        match serde_json::from_str::<TurnRecord>(line) {
            Ok(record) => out.push(record),
            Err(e) => {
                if i == lines.len() - 1 {
                    tracing::warn!("session {session_id}: dropping truncated trailing line: {e}");
                } else {
                    tracing::warn!("session {session_id}: dropping unparseable line {i}: {e}");
                }
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionIndexEntry {
    pub id: String,
    pub started_at: String,
    pub last_turn_at: String,
    pub turn_count: u32,
    pub total_cost_usd: f64,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionIndex {
    pub schema_version: u32,
    pub current: Option<String>,
    pub sessions: Vec<SessionIndexEntry>,
}

impl SessionIndex {
    pub fn empty() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            current: None,
            sessions: Vec::new(),
        }
    }
}

pub fn load_index(workspace: &Path) -> std::io::Result<SessionIndex> {
    match std::fs::read(index_path(workspace)) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_else(|_| SessionIndex::empty())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SessionIndex::empty()),
        Err(e) => Err(e),
    }
}

pub fn save_index(workspace: &Path, index: &SessionIndex) -> std::io::Result<()> {
    let dir = sessions_dir(workspace);
    std::fs::create_dir_all(&dir)?;
    let path = index_path(workspace);
    let tmp = dir.join(format!("index.json.tmp-{}", std::process::id()));
    let json = serde_json::to_vec_pretty(index)?;
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&json)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Updates (or inserts) `record.session_id`'s entry and marks it `current`.
pub fn record_turn(index: &mut SessionIndex, record: &TurnRecord) {
    index.current = Some(record.session_id.clone());
    let cost = record.result.as_ref().and_then(|r| r.total_cost_usd).unwrap_or(0.0);
    let last_turn_at = record.ended_at.clone().unwrap_or_else(|| record.started_at.clone());

    match index.sessions.iter_mut().find(|s| s.id == record.session_id) {
        Some(entry) => {
            entry.last_turn_at = last_turn_at;
            entry.turn_count += 1;
            entry.total_cost_usd += cost;
        }
        None => index.sessions.push(SessionIndexEntry {
            id: record.session_id.clone(),
            started_at: record.started_at.clone(),
            last_turn_at,
            turn_count: 1,
            total_cost_usd: cost,
            title: truncate_title(&record.prompt),
        }),
    }
}

fn truncate_title(prompt: &str) -> String {
    let first_line = prompt.lines().next().unwrap_or("");
    if first_line.chars().count() <= TITLE_CHARS {
        first_line.to_string()
    } else {
        let head: String = first_line.chars().take(TITLE_CHARS).collect();
        format!("{head}\u{2026}")
    }
}

pub fn new_turn_record(
    turn_id: String,
    session_id: String,
    started_at: String,
    prompt: String,
    model: String,
    policy: PermissionPolicySetting,
    argv: Vec<String>,
) -> TurnRecord {
    TurnRecord {
        schema_version: CURRENT_SCHEMA_VERSION,
        turn_id,
        session_id,
        started_at,
        ended_at: None,
        prompt,
        attachments: Vec::new(),
        model,
        policy,
        argv,
        events: Vec::new(),
        assistant_text: String::new(),
        tool_calls: Vec::new(),
        result: None,
        snapshot_before: None,
        changes: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vibe-hw-session-test-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample(turn_id: &str, session_id: &str, prompt: &str) -> TurnRecord {
        let mut r = new_turn_record(
            turn_id.into(),
            session_id.into(),
            "2026-09-20T09:14:02.118Z".into(),
            prompt.into(),
            "sonnet".into(),
            PermissionPolicySetting::Guarded,
            vec!["claude".into(), "-p".into()],
        );
        r.ended_at = Some("2026-09-20T09:15:00.000Z".into());
        r.assistant_text = "Done.".into();
        r.result = Some(TurnResultMeta {
            subtype: "success".into(),
            is_error: false,
            num_turns: 1,
            duration_ms: 100,
            duration_api_ms: 80,
            total_cost_usd: Some(0.01),
            permission_denials: vec![],
        });
        r
    }

    #[test]
    fn missing_session_file_loads_as_empty() {
        let dir = tempdir("missing");
        assert_eq!(load_turns(&dir, "s1").unwrap(), vec![]);
    }

    #[test]
    fn append_then_load_round_trips_in_order() {
        let dir = tempdir("roundtrip");
        append_turn(&dir, &sample("t1", "s1", "first")).unwrap();
        append_turn(&dir, &sample("t2", "s1", "second")).unwrap();

        let turns = load_turns(&dir, "s1").unwrap();
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].turn_id, "t1");
        assert_eq!(turns[1].turn_id, "t2");
    }

    #[test]
    fn append_is_scoped_to_its_own_session_file() {
        let dir = tempdir("scoped");
        append_turn(&dir, &sample("t1", "s1", "a")).unwrap();
        append_turn(&dir, &sample("t2", "s2", "b")).unwrap();

        assert_eq!(load_turns(&dir, "s1").unwrap().len(), 1);
        assert_eq!(load_turns(&dir, "s2").unwrap().len(), 1);
    }

    #[test]
    fn truncated_trailing_line_is_dropped_not_fatal() {
        let dir = tempdir("truncated");
        append_turn(&dir, &sample("t1", "s1", "first")).unwrap();

        // Simulate a crash mid-write on the second line: valid line, then a partial one.
        let path = session_path(&dir, "s1");
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(f, r#"{{"schemaVersion":1,"turnId":"t2","sessionI"#).unwrap();

        let turns = load_turns(&dir, "s1").unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].turn_id, "t1");
    }

    #[test]
    fn record_turn_inserts_a_new_index_entry() {
        let mut index = SessionIndex::empty();
        record_turn(&mut index, &sample("t1", "s1", "MQTT sensor loop please"));

        assert_eq!(index.current.as_deref(), Some("s1"));
        assert_eq!(index.sessions.len(), 1);
        let entry = &index.sessions[0];
        assert_eq!(entry.turn_count, 1);
        assert_eq!(entry.total_cost_usd, 0.01);
        assert_eq!(entry.title, "MQTT sensor loop please");
    }

    #[test]
    fn record_turn_accumulates_on_an_existing_session() {
        let mut index = SessionIndex::empty();
        record_turn(&mut index, &sample("t1", "s1", "first"));
        record_turn(&mut index, &sample("t2", "s1", "second"));

        assert_eq!(index.sessions.len(), 1);
        let entry = &index.sessions[0];
        assert_eq!(entry.turn_count, 2);
        assert!((entry.total_cost_usd - 0.02).abs() < 1e-9);
        // Title stays from the first turn.
        assert_eq!(entry.title, "first");
    }

    #[test]
    fn index_round_trips_through_save_and_load() {
        let dir = tempdir("index-roundtrip");
        let mut index = SessionIndex::empty();
        record_turn(&mut index, &sample("t1", "s1", "hello"));
        save_index(&dir, &index).unwrap();

        let loaded = load_index(&dir).unwrap();
        assert_eq!(loaded, index);
    }

    #[test]
    fn missing_index_loads_as_empty() {
        let dir = tempdir("index-missing");
        let index = load_index(&dir).unwrap();
        assert_eq!(index, SessionIndex::empty());
    }

    #[test]
    fn long_prompt_title_is_truncated_to_60_chars() {
        let long = "x".repeat(100);
        let title = truncate_title(&long);
        assert_eq!(title.chars().count(), TITLE_CHARS + 1); // +1 for the ellipsis
    }
}
