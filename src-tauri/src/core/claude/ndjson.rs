//! The NDJSON stream parser (`CLI-CONTRACT.md` §1.5, `ARCHITECTURE.md` §3 `core/claude`).
//!
//! One JSON object per already-line-split input line (line splitting itself is
//! `core::proc`'s job — see `core::claude::turn`). Malformed or unknown `type`/`subtype`
//! values are logged and skipped, never fatal (`FR-CHAT-2`). A single line over 32 MiB is
//! treated the same way — "output truncated" rather than an unbounded parse attempt
//! (`ARCHITECTURE.md` §3).
//!
//! Deltas (`text_delta`, `thinking_delta`, `input_json_delta`) are buffered into
//! [`Coalescer`] rather than emitted immediately; [`NdjsonParser::feed_line`] flushes it
//! before every structural event so the emitted order always matches the ordering
//! guarantee in `CLI-CONTRACT.md` §1.5 (a block's deltas precede its completion). The
//! ~16 ms timer that flushes the coalescer on its own cadence lives in `core::claude::turn`.

use super::ids::{SessionId, TurnId};
use super::types::ChatEvent;
use serde_json::Value;
use std::collections::HashMap;

const MAX_LINE_BYTES: usize = 32 * 1024 * 1024;
/// `IPC-CONTRACT.md` §10 rule 5: "A single channel message stays under ~256 KB. Tool
/// results larger than that are truncated with `full: None`."
const TOOL_RESULT_FULL_CAP: usize = 256 * 1024;
const SUMMARY_CHARS: usize = 200;

// ---------------------------------------------------------------------------------------
// Coalescer — buffers deltas between flushes (`ARCHITECTURE.md` §8: coalesced in Rust).
// ---------------------------------------------------------------------------------------

#[derive(Default)]
pub struct Coalescer {
    text: Vec<(u32, String)>,
    thinking: Vec<(u32, String)>,
    tool_input: Vec<(String, String)>,
}

impl Coalescer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.thinking.is_empty() && self.tool_input.is_empty()
    }

    fn push_text(&mut self, index: u32, chunk: &str) {
        push_or_append(&mut self.text, index, chunk);
    }

    fn push_thinking(&mut self, index: u32, chunk: &str) {
        push_or_append(&mut self.thinking, index, chunk);
    }

    fn push_tool_input(&mut self, tool_use_id: &str, chunk: &str) {
        push_or_append_keyed(&mut self.tool_input, tool_use_id, chunk);
    }

    /// Drains every pending buffer into `ChatEvent`s, in a stable order (text, then
    /// thinking, then tool-input deltas; insertion order within each).
    pub fn flush(&mut self, turn_id: &TurnId) -> Vec<ChatEvent> {
        let mut out = Vec::with_capacity(self.text.len() + self.thinking.len() + self.tool_input.len());
        for (block_index, text) in self.text.drain(..) {
            out.push(ChatEvent::TextDelta {
                turn_id: turn_id.clone(),
                block_index,
                text,
            });
        }
        for (block_index, text) in self.thinking.drain(..) {
            out.push(ChatEvent::ThinkingDelta {
                turn_id: turn_id.clone(),
                block_index,
                text,
            });
        }
        for (tool_use_id, partial_json) in self.tool_input.drain(..) {
            out.push(ChatEvent::ToolCallInputDelta {
                turn_id: turn_id.clone(),
                tool_use_id,
                partial_json,
            });
        }
        out
    }
}

fn push_or_append(buf: &mut Vec<(u32, String)>, key: u32, chunk: &str) {
    match buf.iter_mut().find(|(k, _)| *k == key) {
        Some((_, s)) => s.push_str(chunk),
        None => buf.push((key, chunk.to_string())),
    }
}

fn push_or_append_keyed(buf: &mut Vec<(String, String)>, key: &str, chunk: &str) {
    match buf.iter_mut().find(|(k, _)| k == key) {
        Some((_, s)) => s.push_str(chunk),
        None => buf.push((key.to_string(), chunk.to_string())),
    }
}

// ---------------------------------------------------------------------------------------
// NdjsonParser
// ---------------------------------------------------------------------------------------

pub struct NdjsonParser {
    turn_id: TurnId,
    /// The content-block index currently open between `content_block_start` and
    /// `content_block_stop`, used to attribute the following `assistant` line's block.
    open_block_index: Option<u32>,
    tool_use_id_by_index: HashMap<u32, String>,
}

impl NdjsonParser {
    pub fn new(turn_id: TurnId) -> Self {
        Self {
            turn_id,
            open_block_index: None,
            tool_use_id_by_index: HashMap::new(),
        }
    }

    pub fn feed_line(&mut self, line: &str, coalescer: &mut Coalescer) -> Vec<ChatEvent> {
        if line.trim().is_empty() {
            return Vec::new();
        }
        if line.len() > MAX_LINE_BYTES {
            tracing::warn!("claude ndjson: line exceeds {MAX_LINE_BYTES} bytes, skipping (output truncated)");
            return Vec::new();
        }
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("claude ndjson: malformed line skipped: {e}");
                return Vec::new();
            }
        };
        match value.get("type").and_then(Value::as_str) {
            Some("system") => self.handle_system(&value, coalescer),
            Some("stream_event") => self.handle_stream_event(&value, coalescer),
            Some("assistant") => self.handle_message(&value, coalescer, "assistant"),
            Some("user") => self.handle_message(&value, coalescer, "user"),
            Some("result") => self.handle_result(&value, coalescer),
            _ => Vec::new(), // unknown/missing `type` — FR-CHAT-2
        }
    }

    fn handle_system(&mut self, value: &Value, coalescer: &mut Coalescer) -> Vec<ChatEvent> {
        match value.get("subtype").and_then(Value::as_str) {
            Some("init") => vec![ChatEvent::SessionReady {
                session_id: SessionId(str_field(value, "session_id")),
                model: str_field(value, "model"),
                tools: str_list(value, "tools"),
                capabilities: str_list(value, "capabilities"),
                mcp_errors: str_list(value, "mcp_server_errors"),
                plugin_errors: str_list(value, "plugin_errors"),
            }],
            Some("api_retry") => {
                let mut out = coalescer.flush(&self.turn_id);
                out.push(ChatEvent::ApiRetry {
                    turn_id: self.turn_id.clone(),
                    attempt: u32_field(value, "attempt"),
                    max_retries: u32_field(value, "max_retries"),
                    retry_delay_ms: u64_field(value, "retry_delay_ms"),
                    error: str_field(value, "error"),
                    error_status: value.get("error_status").and_then(Value::as_u64).map(|v| v as u16),
                });
                out
            }
            // Shape unverified — `SPEC.md` §8 open question 10.
            Some("permission_denied") => {
                let mut out = coalescer.flush(&self.turn_id);
                out.push(ChatEvent::PermissionDenied {
                    turn_id: self.turn_id.clone(),
                    tool: opt_str_field(value, "tool").unwrap_or_else(|| "unknown".to_string()),
                    reason: opt_str_field(value, "reason")
                        .or_else(|| opt_str_field(value, "message"))
                        .unwrap_or_default(),
                });
                out
            }
            // `plugin_install` and any future subtype: no ChatEvent variant renders it yet.
            _ => Vec::new(),
        }
    }

    fn handle_stream_event(&mut self, value: &Value, coalescer: &mut Coalescer) -> Vec<ChatEvent> {
        let Some(event) = value.get("event") else {
            return Vec::new();
        };
        match event.get("type").and_then(Value::as_str) {
            Some("content_block_start") => {
                let index = u32_field(event, "index");
                self.open_block_index = Some(index);
                let block = event.get("content_block");
                if block.and_then(|b| b.get("type")).and_then(Value::as_str) == Some("tool_use") {
                    let tool_use_id = block.and_then(|b| b.get("id")).and_then(Value::as_str).unwrap_or_default().to_string();
                    let name = block.and_then(|b| b.get("name")).and_then(Value::as_str).unwrap_or_default().to_string();
                    self.tool_use_id_by_index.insert(index, tool_use_id.clone());
                    let mut out = coalescer.flush(&self.turn_id);
                    out.push(ChatEvent::ToolCallStarted {
                        turn_id: self.turn_id.clone(),
                        tool_use_id,
                        name,
                        input_preview: String::new(),
                    });
                    out
                } else {
                    Vec::new()
                }
            }
            Some("content_block_delta") => {
                let index = u32_field(event, "index");
                let Some(delta) = event.get("delta") else {
                    return Vec::new();
                };
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        coalescer.push_text(index, delta.get("text").and_then(Value::as_str).unwrap_or(""));
                    }
                    // Field name unverified by analogy with `text_delta` — `SPEC.md` §8 open question 9.
                    Some("thinking_delta") => {
                        coalescer.push_thinking(index, delta.get("thinking").and_then(Value::as_str).unwrap_or(""));
                    }
                    Some("input_json_delta") => {
                        if let Some(tool_use_id) = self.tool_use_id_by_index.get(&index) {
                            coalescer.push_tool_input(
                                tool_use_id,
                                delta.get("partial_json").and_then(Value::as_str).unwrap_or(""),
                            );
                        }
                    }
                    _ => {}
                }
                Vec::new()
            }
            Some("content_block_stop") => {
                let index = u32_field(event, "index");
                if self.open_block_index == Some(index) {
                    self.open_block_index = None;
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn handle_message(&mut self, value: &Value, coalescer: &mut Coalescer, role: &str) -> Vec<ChatEvent> {
        let message = value.get("message");
        let parent_tool_use_id = message.and_then(|m| m.get("parent_tool_use_id")).and_then(Value::as_str);
        let content = message
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let mut out = Vec::new();
        for item in &content {
            if let Some(parent) = parent_tool_use_id {
                out.extend(coalescer.flush(&self.turn_id));
                out.push(ChatEvent::SubagentMessage {
                    turn_id: self.turn_id.clone(),
                    parent_tool_use_id: parent.to_string(),
                    role: role.to_string(),
                    text: flatten_subagent_block(item),
                });
                continue;
            }

            match item.get("type").and_then(Value::as_str) {
                Some("text") if role == "assistant" => {
                    let block_index = self.open_block_index.unwrap_or(0);
                    let text = item.get("text").and_then(Value::as_str).unwrap_or("").to_string();
                    out.extend(coalescer.flush(&self.turn_id));
                    out.push(ChatEvent::TextBlock {
                        turn_id: self.turn_id.clone(),
                        block_index,
                        text,
                    });
                }
                // No `ThinkingBlock` structural event — the deltas already carried the
                // content; just make sure they're flushed in order.
                Some("thinking") if role == "assistant" => out.extend(coalescer.flush(&self.turn_id)),
                Some("tool_use") if role == "assistant" => {
                    let tool_use_id = item.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
                    let name = item.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
                    let input = item.get("input").cloned().unwrap_or(Value::Object(Default::default()));
                    out.extend(coalescer.flush(&self.turn_id));
                    out.push(ChatEvent::ToolCallCompleted {
                        turn_id: self.turn_id.clone(),
                        tool_use_id,
                        name,
                        input,
                    });
                }
                Some("tool_result") if role == "user" => {
                    let tool_use_id = item.get("tool_use_id").and_then(Value::as_str).unwrap_or_default().to_string();
                    let (text, is_error) = tool_result_text(item);
                    out.extend(coalescer.flush(&self.turn_id));
                    out.push(ChatEvent::ToolResult {
                        turn_id: self.turn_id.clone(),
                        tool_use_id,
                        is_error,
                        summary: truncate_chars(&text, SUMMARY_CHARS),
                        full: if text.len() <= TOOL_RESULT_FULL_CAP { Some(text) } else { None },
                    });
                }
                _ => {} // unknown content-block type — FR-CHAT-2
            }
        }
        out
    }

    fn handle_result(&mut self, value: &Value, coalescer: &mut Coalescer) -> Vec<ChatEvent> {
        let mut out = coalescer.flush(&self.turn_id);
        out.push(ChatEvent::Result {
            turn_id: self.turn_id.clone(),
            session_id: SessionId(str_field(value, "session_id")),
            subtype: str_field(value, "subtype"),
            is_error: value.get("is_error").and_then(Value::as_bool).unwrap_or(false),
            num_turns: u32_field(value, "num_turns"),
            duration_ms: u64_field(value, "duration_ms"),
            duration_api_ms: u64_field(value, "duration_api_ms"),
            total_cost_usd: value.get("total_cost_usd").and_then(Value::as_f64),
            result_text: opt_str_field(value, "result"),
            permission_denials: str_list(value, "permission_denials"),
        });
        out
    }
}

// ---------------------------------------------------------------------------------------
// Defensive extraction helpers — every field here is "unverified in general" per
// `CLI-CONTRACT.md`'s own caveats, so read leniently and never panic on a missing/
// wrong-typed field (FR-CHAT-2).
// ---------------------------------------------------------------------------------------

fn str_field(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn opt_str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

fn u32_field(v: &Value, key: &str) -> u32 {
    u64_field(v, key) as u32
}

fn u64_field(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// Converts a JSON array field to `Vec<String>`, tolerating elements that are already
/// strings (the common case) or arbitrary JSON (stringified as a fallback) — the shape of
/// `mcp_server_errors[]`/`plugin_errors[]`/`permission_denials[]` isn't fully specified.
fn str_list(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|item| match item {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let head: String = text.chars().take(max).collect();
        format!("{head}\u{2026}")
    }
}

/// A `tool_result` content item's `content` is either a bare string or an array of
/// content blocks (typically `[{type:"text",text:"..."}]`).
fn tool_result_text(item: &Value) -> (String, bool) {
    let is_error = item.get("is_error").and_then(Value::as_bool).unwrap_or(false);
    let text = match item.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    (text, is_error)
}

/// `ChatEvent::SubagentMessage` is a flat `{ role, text }` — see `SPEC.md` §8 open
/// question 11 for why this flattens rather than emitting structured sub-events.
fn flatten_subagent_block(item: &Value) -> String {
    match item.get("type").and_then(Value::as_str) {
        Some("text") => item.get("text").and_then(Value::as_str).unwrap_or("").to_string(),
        Some("tool_use") => {
            let name = item.get("name").and_then(Value::as_str).unwrap_or("?");
            let input = item.get("input").cloned().unwrap_or(Value::Null);
            format!("Tool: {name}({input})")
        }
        Some("tool_result") => {
            let (text, _) = tool_result_text(item);
            format!("Result: {text}")
        }
        Some(other) => format!("[{other}]"),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> NdjsonParser {
        NdjsonParser::new(TurnId("t1".into()))
    }

    #[test]
    fn malformed_line_is_skipped_not_fatal() {
        let mut p = parser();
        let mut c = Coalescer::new();
        assert!(p.feed_line("{ not json", &mut c).is_empty());
        assert!(p.feed_line("", &mut c).is_empty());
    }

    #[test]
    fn unknown_type_is_skipped() {
        let mut p = parser();
        let mut c = Coalescer::new();
        assert!(p
            .feed_line(r#"{"type":"some_future_type","foo":"bar"}"#, &mut c)
            .is_empty());
    }

    #[test]
    fn unknown_system_subtype_is_skipped() {
        let mut p = parser();
        let mut c = Coalescer::new();
        assert!(p
            .feed_line(r#"{"type":"system","subtype":"plugin_install","status":"started"}"#, &mut c)
            .is_empty());
    }

    #[test]
    fn huge_line_is_skipped_not_parsed() {
        let mut p = parser();
        let mut c = Coalescer::new();
        let huge = format!(r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"{}"}}]}}}}"#, "x".repeat(MAX_LINE_BYTES + 10));
        assert!(p.feed_line(&huge, &mut c).is_empty());
    }

    #[test]
    fn session_ready_from_system_init() {
        let mut p = parser();
        let mut c = Coalescer::new();
        let events = p.feed_line(
            r#"{"type":"system","subtype":"init","session_id":"s1","model":"claude-opus-5","tools":["Read","Edit"],"mcp_servers":[],"mcp_server_errors":[],"plugins":[],"plugin_errors":[],"capabilities":["partial-messages"]}"#,
            &mut c,
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            ChatEvent::SessionReady { session_id, model, tools, capabilities, .. } => {
                assert_eq!(session_id.0, "s1");
                assert_eq!(model, "claude-opus-5");
                assert_eq!(tools, &vec!["Read".to_string(), "Edit".to_string()]);
                assert_eq!(capabilities, &vec!["partial-messages".to_string()]);
            }
            other => panic!("expected SessionReady, got {other:?}"),
        }
    }

    #[test]
    fn text_deltas_are_buffered_then_flushed_before_the_text_block() {
        let mut p = parser();
        let mut c = Coalescer::new();

        assert!(p
            .feed_line(r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}"#, &mut c)
            .is_empty());
        assert!(p
            .feed_line(r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}}"#, &mut c)
            .is_empty());
        assert!(p
            .feed_line(r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" world"}}}"#, &mut c)
            .is_empty());
        // Deltas are buffered, not emitted — nothing forwarded yet.
        assert!(!c.is_empty());

        // The complete block arrives next (per CLI-CONTRACT.md ordering guarantee) —
        // feeding it must flush the buffered deltas first, then the block itself.
        let events = p.feed_line(
            r#"{"type":"assistant","message":{"role":"assistant","parent_tool_use_id":null,"content":[{"type":"text","text":"Hello world"}]}}"#,
            &mut c,
        );
        assert_eq!(events.len(), 2);
        match &events[0] {
            ChatEvent::TextDelta { block_index, text, .. } => {
                assert_eq!(*block_index, 0);
                assert_eq!(text, "Hello world");
            }
            other => panic!("expected TextDelta first, got {other:?}"),
        }
        match &events[1] {
            ChatEvent::TextBlock { block_index, text, .. } => {
                assert_eq!(*block_index, 0);
                assert_eq!(text, "Hello world");
            }
            other => panic!("expected TextBlock second, got {other:?}"),
        }
        assert!(c.is_empty());
    }

    #[test]
    fn full_tool_call_lifecycle_in_order() {
        let mut p = parser();
        let mut c = Coalescer::new();

        let started = p.feed_line(
            r#"{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"Write","input":{}}}}"#,
            &mut c,
        );
        assert_eq!(started.len(), 1);
        assert!(matches!(&started[0], ChatEvent::ToolCallStarted { tool_use_id, name, .. } if tool_use_id == "toolu_1" && name == "Write"));

        assert!(p
            .feed_line(r#"{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"file_path\":"}}}"#, &mut c)
            .is_empty());

        let completed = p.feed_line(
            r#"{"type":"assistant","message":{"role":"assistant","parent_tool_use_id":null,"content":[{"type":"tool_use","id":"toolu_1","name":"Write","input":{"file_path":"src/main.cpp"}}]}}"#,
            &mut c,
        );
        assert_eq!(completed.len(), 2);
        assert!(matches!(&completed[0], ChatEvent::ToolCallInputDelta { tool_use_id, .. } if tool_use_id == "toolu_1"));
        assert!(matches!(&completed[1], ChatEvent::ToolCallCompleted { tool_use_id, .. } if tool_use_id == "toolu_1"));

        let result = p.feed_line(
            r#"{"type":"user","message":{"role":"user","parent_tool_use_id":null,"content":[{"type":"tool_result","tool_use_id":"toolu_1","is_error":false,"content":[{"type":"text","text":"File written successfully."}]}]}}"#,
            &mut c,
        );
        assert_eq!(result.len(), 1);
        match &result[0] {
            ChatEvent::ToolResult { tool_use_id, is_error, summary, full, .. } => {
                assert_eq!(tool_use_id, "toolu_1");
                assert!(!is_error);
                assert_eq!(summary, "File written successfully.");
                assert_eq!(full.as_deref(), Some("File written successfully."));
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn api_retry_and_permission_denied_flush_pending_deltas_first() {
        let mut p = parser();
        let mut c = Coalescer::new();
        p.feed_line(r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}"#, &mut c);
        p.feed_line(r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"thinking..."}}}"#, &mut c);

        let events = p.feed_line(
            r#"{"type":"system","subtype":"api_retry","attempt":1,"max_retries":5,"retry_delay_ms":2000,"error":"overloaded","error_status":529}"#,
            &mut c,
        );
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], ChatEvent::TextDelta { .. }));
        match &events[1] {
            ChatEvent::ApiRetry { attempt, max_retries, error, error_status, .. } => {
                assert_eq!(*attempt, 1);
                assert_eq!(*max_retries, 5);
                assert_eq!(error, "overloaded");
                assert_eq!(*error_status, Some(529));
            }
            other => panic!("expected ApiRetry, got {other:?}"),
        }

        let denied = p.feed_line(r#"{"type":"system","subtype":"permission_denied","tool":"Bash","reason":"not in allowedTools"}"#, &mut c);
        assert_eq!(denied.len(), 1);
        match &denied[0] {
            ChatEvent::PermissionDenied { tool, reason, .. } => {
                assert_eq!(tool, "Bash");
                assert_eq!(reason, "not in allowedTools");
            }
            other => panic!("expected PermissionDenied, got {other:?}"),
        }
    }

    #[test]
    fn thinking_delta_has_no_structural_block_event() {
        let mut p = parser();
        let mut c = Coalescer::new();
        p.feed_line(r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking"}}}"#, &mut c);
        p.feed_line(r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"hmm"}}}"#, &mut c);

        let events = p.feed_line(
            r#"{"type":"assistant","message":{"role":"assistant","parent_tool_use_id":null,"content":[{"type":"thinking","thinking":"hmm"}]}}"#,
            &mut c,
        );
        // Only the flushed delta — no structural "ThinkingBlock" (doesn't exist).
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ChatEvent::ThinkingDelta { text, .. } if text == "hmm"));
    }

    #[test]
    fn subagent_messages_are_flattened_by_role() {
        let mut p = parser();
        let mut c = Coalescer::new();
        let events = p.feed_line(
            r#"{"type":"assistant","message":{"role":"assistant","parent_tool_use_id":"toolu_task","content":[{"type":"text","text":"Investigating..."}]}}"#,
            &mut c,
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            ChatEvent::SubagentMessage { parent_tool_use_id, role, text, .. } => {
                assert_eq!(parent_tool_use_id, "toolu_task");
                assert_eq!(role, "assistant");
                assert_eq!(text, "Investigating...");
            }
            other => panic!("expected SubagentMessage, got {other:?}"),
        }
    }

    #[test]
    fn result_event_extracts_all_fields() {
        let mut p = parser();
        let mut c = Coalescer::new();
        let events = p.feed_line(
            r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":4213,"duration_api_ms":3190,"num_turns":1,"result":"Done.","session_id":"s1","total_cost_usd":0.0431,"permission_denials":[]}"#,
            &mut c,
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            ChatEvent::Result {
                session_id,
                subtype,
                is_error,
                num_turns,
                duration_ms,
                duration_api_ms,
                total_cost_usd,
                result_text,
                permission_denials,
                ..
            } => {
                assert_eq!(session_id.0, "s1");
                assert_eq!(subtype, "success");
                assert!(!is_error);
                assert_eq!(*num_turns, 1);
                assert_eq!(*duration_ms, 4213);
                assert_eq!(*duration_api_ms, 3190);
                assert_eq!(*total_cost_usd, Some(0.0431));
                assert_eq!(result_text.as_deref(), Some("Done."));
                assert!(permission_denials.is_empty());
            }
            other => panic!("expected Result, got {other:?}"),
        }
    }

    #[test]
    fn large_tool_result_is_truncated_with_no_full_copy() {
        let mut p = parser();
        let mut c = Coalescer::new();
        let big = "x".repeat(TOOL_RESULT_FULL_CAP + 1);
        let line = format!(
            r#"{{"type":"user","message":{{"role":"user","parent_tool_use_id":null,"content":[{{"type":"tool_result","tool_use_id":"t1","is_error":false,"content":[{{"type":"text","text":"{big}"}}]}}]}}}}"#
        );
        let events = p.feed_line(&line, &mut c);
        match &events[0] {
            ChatEvent::ToolResult { full, summary, .. } => {
                assert!(full.is_none());
                assert!(summary.chars().count() <= SUMMARY_CHARS + 1);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }
}
