# IPC-CONTRACT.md — Rust ⇄ React interface

Rust is the source of truth. Generate `src/lib/bindings.ts` with `ts-rs` (or `specta`)
and fail CI on drift. The TypeScript below is what the generator must produce — do not
hand-maintain it.

Conventions:
- All types `#[serde(rename_all = "camelCase")]`.
- All commands return `Result<T, AppError>`; `AppError` is the enum in
  `ARCHITECTURE.md` §6 and is externally tagged on `code`.
- Streaming commands take an `on_event: Channel<T>` argument and return immediately with
  a handle id. The channel closes when the operation ends.
- Ids are newtypes over `String` (UUID v4) so they cannot be mixed up.

---

## 1. Shared types

```rust
pub struct WorkspaceId(pub String);
pub struct SessionId(pub String);     // Claude session UUID
pub struct TurnId(pub String);
pub struct ProcId(pub String);
pub struct SnapshotId(pub String);    // git oid
```

```ts
export type WorkspaceId = string;
export type SessionId   = string;
export type TurnId      = string;
export type ProcId      = string;
export type SnapshotId  = string;
```

---

## 2. Doctor & toolchain

```rust
#[derive(Serialize)] #[serde(rename_all = "camelCase", tag = "status")]
pub enum ProbeResult {
    Ok       { version: String, path: Option<String>, detail: Option<String> },
    Missing  { install_available: bool },
    Degraded { reason: String, remediation: Option<Remediation> },
    Error    { detail: String },
    Probing,
}

#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct Remediation {
    pub kind: RemediationKind,   // InstallClaude | InstallPio | AuthenticateClaude
                                 // | InstallUdevRules | OpenUrl | ShowCommand
    pub label: String,
    pub command_preview: Option<Vec<String>>,   // shown to the user before running
    pub url: Option<String>,
}

#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub claude_binary: ProbeResult,
    pub claude_auth: ProbeResult,
    pub claude_capabilities: Vec<String>,   // from system/init, when known
    pub pio_binary: ProbeResult,
    pub pio_core_dir: ProbeResult,
    pub python: ProbeResult,
    pub network_registry: ProbeResult,
    pub serial_permissions: ProbeResult,
    pub git: ProbeResult,
    pub probed_at: String,                  // RFC3339
}
```

### Commands

| Command | Signature |
|---|---|
| `doctor_run` | `(force: bool) -> DoctorReport` |
| `doctor_get_cached` | `() -> Option<DoctorReport>` |
| `toolchain_install` | `(kind: RemediationKind, on_event: Channel<InstallEvent>) -> ProcId` |
| `toolchain_set_path` | `(tool: "claude" \| "pio", path: String) -> ProbeResult` |
| `toolchain_open_auth_terminal` | `() -> ()` — launches an interactive `claude` in the OS terminal |
| `diagnostics_export` | `() -> String` — path to the written zip |

```rust
#[derive(Serialize)] #[serde(rename_all = "camelCase", tag = "type", content = "data")]
pub enum InstallEvent {
    Started  { argv: Vec<String> },
    Line     { stream: StdStream, text: String },
    Progress { message: String, fraction: Option<f32> },
    Finished { success: bool, exit_code: i32 },
}
pub enum StdStream { Stdout, Stderr }
```

---

## 3. Projects

```rust
#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct BoardBrief {
    pub id: String, pub name: String, pub platform: String,
    pub mcu: String, pub fcpu: u64, pub ram: u64, pub rom: u64,
    pub frameworks: Vec<String>, pub vendor: String, pub url: String,
    pub connectivity: Vec<String>,          // [] when absent
    pub debug_tools: Vec<String>,           // [] when absent
}

#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct ProjectEntry {
    pub id: WorkspaceId, pub name: String, pub path: String,
    pub board_id: Option<String>, pub active_env: Option<String>,
    pub last_opened: Option<String>, pub last_build_ok: Option<bool>,
    pub exists: bool, pub trusted: bool,
}

#[derive(Deserialize)] #[serde(rename_all = "camelCase")]
pub struct CreateProjectRequest {
    pub parent_dir: String, pub name: String,
    pub board_id: String, pub framework: String,
    pub sample_code: bool, pub init_git: bool, pub generate_claude_md: bool,
    pub extra_options: Vec<(String, String)>,   // -O name=value
}
```

| Command | Signature |
|---|---|
| `boards_list` | `(refresh: bool, installed_only: bool) -> Vec<BoardBrief>` |
| `project_create` | `(req: CreateProjectRequest, on_event: Channel<ProcEvent>) -> ProcId` |
| `project_open` | `(path: String) -> ProjectEntry` |
| `project_list` | `() -> Vec<ProjectEntry>` |
| `project_forget` | `(id: WorkspaceId) -> ()` |
| `project_trust` | `(id: WorkspaceId) -> ()` |
| `project_scan_trust` | `(path: String) -> TrustScan` — hooks, MCP servers, agents found |
| `project_set_env` | `(id: WorkspaceId, env: String) -> ()` |
| `project_open_in_editor` | `(id: WorkspaceId, file: Option<String>, line: Option<u32>) -> ()` |
| `project_reveal` | `(id: WorkspaceId) -> ()` |
| `project_regenerate_claude_md` | `(id: WorkspaceId) -> String` — returns the new content |

---

## 4. Claude turns

```rust
#[derive(Serialize, Deserialize)] #[serde(rename_all = "camelCase")]
pub enum PermissionPolicy { Guarded, Assisted, Unrestricted }

#[derive(Deserialize)] #[serde(rename_all = "camelCase")]
pub struct TurnRequest {
    pub workspace: WorkspaceId,
    pub prompt: String,
    pub attachments: Vec<String>,     // relative paths under .vibe/attachments
    pub policy: Option<PermissionPolicy>,   // None = use setting
    pub model: Option<String>,
}
```

### `ChatEvent` — the streaming payload

```rust
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase",
        tag = "type", content = "data")]
pub enum ChatEvent {
    /// Emitted once we have parsed system/init.
    SessionReady { session_id: SessionId, model: String, tools: Vec<String>,
                   capabilities: Vec<String>, mcp_errors: Vec<String>,
                   plugin_errors: Vec<String> },

    /// Coalesced assistant prose. `blockIndex` groups deltas into content blocks.
    TextDelta { turn_id: TurnId, block_index: u32, text: String },

    /// A complete assistant content block arrived.
    TextBlock { turn_id: TurnId, block_index: u32, text: String },

    /// Thinking blocks, when present, rendered collapsed by default.
    ThinkingDelta { turn_id: TurnId, block_index: u32, text: String },

    /// A tool call began. `input` may be partial while streaming.
    ToolCallStarted { turn_id: TurnId, tool_use_id: String, name: String,
                      input_preview: String },
    ToolCallInputDelta { turn_id: TurnId, tool_use_id: String, partial_json: String },
    ToolCallCompleted { turn_id: TurnId, tool_use_id: String, name: String,
                        input: serde_json::Value },
    ToolResult { turn_id: TurnId, tool_use_id: String, is_error: bool,
                 summary: String, full: Option<String> },

    /// Subagent attribution: non-null parentToolUseId on assistant/user messages.
    SubagentMessage { turn_id: TurnId, parent_tool_use_id: String, role: String,
                      text: String },

    ApiRetry { turn_id: TurnId, attempt: u32, max_retries: u32,
               retry_delay_ms: u64, error: String, error_status: Option<u16> },

    PermissionDenied { turn_id: TurnId, tool: String, reason: String },

    CompactBoundary { turn_id: TurnId },

    /// Final. Always emitted, even on failure or interruption.
    Result { turn_id: TurnId, session_id: SessionId, subtype: String,
             is_error: bool, num_turns: u32, duration_ms: u64,
             duration_api_ms: u64, total_cost_usd: Option<f64>,
             result_text: Option<String>, permission_denials: Vec<String> },

    /// Emitted after Result, once the snapshot diff has been computed.
    ChangesComputed { turn_id: TurnId, snapshot: SnapshotId, changes: Vec<FileChange> },

    /// Process-level failure with no usable Result event.
    Failed { turn_id: TurnId, error: AppError },
}
```

```ts
export type ChatEvent =
  | { type: 'sessionReady';       data: { sessionId: SessionId; model: string; tools: string[]; capabilities: string[]; mcpErrors: string[]; pluginErrors: string[] } }
  | { type: 'textDelta';          data: { turnId: TurnId; blockIndex: number; text: string } }
  | { type: 'textBlock';          data: { turnId: TurnId; blockIndex: number; text: string } }
  | { type: 'thinkingDelta';      data: { turnId: TurnId; blockIndex: number; text: string } }
  | { type: 'toolCallStarted';    data: { turnId: TurnId; toolUseId: string; name: string; inputPreview: string } }
  | { type: 'toolCallInputDelta'; data: { turnId: TurnId; toolUseId: string; partialJson: string } }
  | { type: 'toolCallCompleted';  data: { turnId: TurnId; toolUseId: string; name: string; input: unknown } }
  | { type: 'toolResult';         data: { turnId: TurnId; toolUseId: string; isError: boolean; summary: string; full: string | null } }
  | { type: 'subagentMessage';    data: { turnId: TurnId; parentToolUseId: string; role: string; text: string } }
  | { type: 'apiRetry';           data: { turnId: TurnId; attempt: number; maxRetries: number; retryDelayMs: number; error: string; errorStatus: number | null } }
  | { type: 'permissionDenied';   data: { turnId: TurnId; tool: string; reason: string } }
  | { type: 'compactBoundary';    data: { turnId: TurnId } }
  | { type: 'result';             data: { turnId: TurnId; sessionId: SessionId; subtype: string; isError: boolean; numTurns: number; durationMs: number; durationApiMs: number; totalCostUsd: number | null; resultText: string | null; permissionDenials: string[] } }
  | { type: 'changesComputed';    data: { turnId: TurnId; snapshot: SnapshotId; changes: FileChange[] } }
  | { type: 'failed';             data: { turnId: TurnId; error: AppError } };
```

### Commands

| Command | Signature |
|---|---|
| `claude_send_turn` | `(req: TurnRequest, on_event: Channel<ChatEvent>) -> TurnId` |
| `claude_stop_turn` | `(turn_id: TurnId, hard: bool) -> ()` — `hard=false` → SIGINT, `true` → SIGTERM |
| `claude_new_session` | `(workspace: WorkspaceId) -> SessionId` |
| `claude_history` | `(workspace: WorkspaceId, limit: u32, before: Option<TurnId>) -> Vec<TurnRecord>` |
| `claude_attach_file` | `(workspace: WorkspaceId, source_path: String) -> String` — returns the relative path to reference |

### Frontend usage

```ts
import { invoke, Channel } from '@tauri-apps/api/core';

const onEvent = new Channel<ChatEvent>();
onEvent.onmessage = (e) => chatStore.getState().apply(e);

const turnId = await invoke<TurnId>('claude_send_turn', {
  req: { workspace, prompt, attachments: [], policy: null, model: null },
  onEvent,
});
```

---

## 5. Pipeline: build, upload, targets

```rust
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase",
        tag = "type", content = "data")]
pub enum ProcEvent {
    Started { proc_id: ProcId, label: String, argv: Vec<String> },
    /// Batched: up to N lines per message (see ARCHITECTURE §8).
    Lines   { proc_id: ProcId, lines: Vec<LogLine> },
    Defect  { proc_id: ProcId, defect: Defect },
    Size    { proc_id: ProcId, usage: SizeUsage },
    Stage   { proc_id: ProcId, stage: String },   // "Compiling", "Linking", "Writing at 0x10000"
    /// `M9`/`FR-BUILD-10`: one per environment `pipeline_test` reports on — synthesized
    /// from `pio test --json-output`'s single end-of-run JSON blob, the same way `Defect`
    /// is synthesized from `pio check --json-output`'s (`SPEC.md` §8 open question 39).
    TestResult{ proc_id: ProcId, suite: TestSuite },
    Finished{ proc_id: ProcId, success: bool, exit_code: i32, duration_ms: u64 },
}

#[derive(Clone, Serialize)] #[serde(rename_all = "camelCase")]
pub enum TestStatus { Passed, Failed, Errored, Skipped }

#[derive(Clone, Serialize)] #[serde(rename_all = "camelCase")]
pub struct TestCaseResult {
    pub name: String, pub status: TestStatus, pub message: Option<String>,
    pub duration: f64, pub file: Option<String>, pub line: Option<u32>,
}

#[derive(Clone, Serialize)] #[serde(rename_all = "camelCase")]
pub struct TestSuite {
    pub env_name: String, pub test_name: String, pub status: TestStatus,
    pub duration: f64, pub cases: Vec<TestCaseResult>,
}

#[derive(Clone, Serialize)] #[serde(rename_all = "camelCase")]
pub struct LogLine { pub stream: StdStream, pub text: String, pub ts_ms: u64 }

#[derive(Clone, Serialize)] #[serde(rename_all = "camelCase")]
pub struct Defect {
    pub file: String, pub line: u32, pub column: Option<u32>,
    pub severity: Severity,       // Error | Warning | Note
    pub message: String, pub source: DefectSource,   // Compiler | Linker | Check
    pub raw: String,
}

#[derive(Clone, Serialize)] #[serde(rename_all = "camelCase")]
pub struct SizeUsage {
    pub ram_used: u64, pub ram_total: u64,
    pub flash_used: u64, pub flash_total: u64,
    pub ram_delta: Option<i64>, pub flash_delta: Option<i64>,
}
```

| Command | Signature |
|---|---|
| `pipeline_build` | `(workspace: WorkspaceId, on_event: Channel<ProcEvent>) -> ProcId` |
| `pipeline_upload` | `(workspace: WorkspaceId, on_event: Channel<ProcEvent>) -> ProcId` |
| `pipeline_run_target` | `(workspace: WorkspaceId, target: String, on_event: Channel<ProcEvent>) -> ProcId` |
| `pipeline_check` | `(workspace: WorkspaceId, on_event: Channel<ProcEvent>) -> ProcId` |
| `pipeline_test` | `(workspace: WorkspaceId, on_event: Channel<ProcEvent>) -> ProcId` |
| `pipeline_stop` | `(proc_id: ProcId) -> ()` |
| `pipeline_targets` | `(workspace: WorkspaceId, refresh: bool) -> Vec<String>` |
| `pipeline_state` | `(workspace: WorkspaceId) -> PipelineState` |

Emitted globally (not via channel): `pipeline://state` carrying
`{ workspace, state, since }` on every transition of the §4.1 state machine.

---

## 6. Devices, telemetry, monitor

```rust
#[derive(Clone, Serialize)] #[serde(rename_all = "camelCase")]
pub struct SerialDevice {
    pub port: String, pub description: String, pub hwid: String,
    pub vid: Option<String>, pub pid: Option<String>, pub serial: Option<String>,
    pub known_bridge: Option<String>,   // "CH340" | "CP210x" | "FTDI" | "USB CDC"
}

#[derive(Clone, Serialize)] #[serde(rename_all = "camelCase")]
pub struct Telemetry {
    pub adapter: String,                  // "esp" | "generic"
    pub connected: bool,
    pub port: Option<String>,
    pub chip: Option<String>,
    pub flash_size: Option<String>,
    pub flash_vendor: Option<String>,
    pub mac: Option<String>,
    pub board: Option<BoardBrief>,
    pub unavailable_reason: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase",
        tag = "type", content = "data")]
pub enum MonitorEvent {
    Opened { port: String, baud: u32 },
    Data   { chunk: String, ts_ms: u64 },     // already decoded per `monitor_encoding`
    Preempted { by: String },
    Reattached { port: String },
    Closed { reason: String },
    Error  { error: AppError },
}
```

| Command | Signature |
|---|---|
| `device_list` | `() -> Vec<SerialDevice>` |
| `device_select` | `(workspace: WorkspaceId, port: String) -> ()` |
| `device_telemetry` | `(workspace: WorkspaceId, refresh: bool) -> Telemetry` |
| `monitor_start` | `(workspace: WorkspaceId, on_event: Channel<MonitorEvent>) -> ProcId` |
| `monitor_stop` | `(workspace: WorkspaceId) -> ()` |
| `monitor_send` | `(workspace: WorkspaceId, text: String) -> ()` — EOL appended per settings |
| `monitor_save_log` | `(workspace: WorkspaceId, path: String) -> ()` |
| `monitor_open_external` | `(workspace: WorkspaceId) -> ()` |

Global event `device://changed` carries `{ added: SerialDevice[], removed: string[] }`.

---

## 7. `platformio.ini` and packages

```rust
#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct IniOptionSchema {
    pub key: String,          // "env.build_flags"
    pub scope: String, pub group: String, pub name: String,
    pub description: String,
    pub r#type: String,       // "string" | "integer" | "choice" | "path" | "boolean"
    pub multiple: bool,
    pub default: Option<serde_json::Value>,
    pub choices: Option<Vec<String>>,
    pub min: Option<f64>, pub max: Option<f64>,
    pub sysenvvar: Option<String>,
}

#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct IniDocument {
    pub raw: String,
    pub sections: Vec<IniSection>,
    pub mtime_ms: u64,                    // for the changed-on-disk guard
}

#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct IniSection {
    pub name: String,                     // "platformio" | "env" | "env:esp32dev"
    pub declared: Vec<IniEntry>,
    pub effective: Vec<IniEntry>,         // from `pio project config --json-output`
}

#[derive(Serialize, Deserialize)] #[serde(rename_all = "camelCase")]
pub struct IniEntry {
    pub name: String,
    pub values: Vec<String>,              // single-valued options carry one element
    pub inherited_from: Option<String>,   // Some("env") when not declared here
}

#[derive(Deserialize)] #[serde(rename_all = "camelCase")]
pub enum IniEdit {
    Set    { section: String, name: String, values: Vec<String> },
    Remove { section: String, name: String },
    AddSection    { name: String },
    RemoveSection { name: String },
    RenameSection { from: String, to: String },
}
```

| Command | Signature |
|---|---|
| `ini_schema` | `() -> Vec<IniOptionSchema>` — cached per PIO version |
| `ini_read` | `(workspace: WorkspaceId) -> IniDocument` |
| `ini_apply` | `(workspace: WorkspaceId, edits: Vec<IniEdit>, expected_mtime_ms: u64) -> IniDocument` |
| `ini_write_raw` | `(workspace: WorkspaceId, raw: String, expected_mtime_ms: u64) -> IniDocument` |
| `ini_lint` | `(workspace: WorkspaceId) -> LintReport` |
| `ini_template_save` | `(workspace: WorkspaceId, name: String) -> ()` |
| `ini_template_list` | `() -> Vec<IniTemplate>` |
| `ini_template_apply` | `(workspace: WorkspaceId, name: String) -> IniDocument` |

`ini_apply` / `ini_write_raw` return `AppError::IniChangedOnDisk` when
`expected_mtime_ms` does not match — the UI must offer Reload / Overwrite rather than
clobbering an edit Fay made in her editor.

### Packages

```rust
#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct RegistryPackage {
    pub owner: String, pub name: String, pub r#type: String,   // library|platform|tool
    pub tier: String, pub description: String,
    pub version: String, pub released_at: Option<String>,
    pub installed_version: Option<String>,
}
```

| Command | Signature |
|---|---|
| `pkg_search` | `(query: String, qualifiers: Vec<(String,String)>, page: u32, sort: Option<String>) -> PackagePage` |
| `pkg_install` | `(workspace: WorkspaceId, spec: String, kind: PkgKind, on_event: Channel<ProcEvent>) -> ProcId` |
| `pkg_uninstall` | `(workspace: WorkspaceId, spec: String, kind: PkgKind, on_event: Channel<ProcEvent>) -> ProcId` |
| `pkg_installed` | `(workspace: WorkspaceId) -> Vec<InstalledPackage>` |
| `pkg_outdated` | `(workspace: WorkspaceId) -> Vec<OutdatedPackage>` |
| `pio_settings_get` | `() -> Vec<PioSetting>` |
| `pio_settings_set` | `(name: String, value: String) -> Vec<PioSetting>` |
| `pio_system_prune` | `(on_event: Channel<ProcEvent>) -> ProcId` |

---

## 8. Snapshots and changes

```rust
#[derive(Clone, Serialize)] #[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub status: ChangeStatus,      // Added | Modified | Deleted | Renamed
    pub additions: u32, pub deletions: u32,
    pub outside_expected_dirs: bool,   // drives the FR-SAFE-4 warning
}

#[derive(Clone, Serialize)] #[serde(rename_all = "camelCase")]
pub struct FileDiff { pub path: String, pub before: Option<String>, pub after: Option<String> }
```

| Command | Signature |
|---|---|
| `changes_for_turn` | `(workspace: WorkspaceId, turn_id: TurnId) -> Vec<FileChange>` |
| `changes_diff` | `(workspace: WorkspaceId, turn_id: TurnId, path: String) -> FileDiff` |
| `changes_revert_file` | `(workspace: WorkspaceId, turn_id: TurnId, path: String) -> ()` |
| `changes_revert_turn` | `(workspace: WorkspaceId, turn_id: TurnId) -> ()` |
| `changes_reset_to_last_good_build` | `(workspace: WorkspaceId) -> SnapshotId` |
| `file_read` | `(workspace: WorkspaceId, path: String) -> String` — read-only viewer |

---

## 9. Settings

| Command | Signature |
|---|---|
| `settings_get_global` | `() -> GlobalSettings` |
| `settings_set_global` | `(patch: GlobalSettingsPatch) -> GlobalSettings` |
| `settings_get_project` | `(workspace: WorkspaceId) -> ProjectSettings` |
| `settings_set_project` | `(workspace: WorkspaceId, patch: ProjectSettingsPatch) -> ProjectSettings` |
| `app_info_get` | `() -> AppInfo { version, gitSha }` (`M10`/`NFR-D3`) |

Schemas in [`DATA-MODEL.md`](./DATA-MODEL.md) §3–§4. Global event `settings://changed`
notifies all windows.

`GlobalSettings` gained two `M10` sections: `updates: { checkForUpdates: bool }`
(`NFR-D2`'s opt-out) and a top-level `privacyNoticeAcknowledged: bool` (`NFR-S2`'s
first-run disclosure). Update checking/installing itself isn't a custom command — the
frontend calls `@tauri-apps/plugin-updater`'s `check()`/`Update.downloadAndInstall()` and
`@tauri-apps/plugin-process`'s `relaunch()` directly, the same way `@tauri-apps/
plugin-dialog` is already called directly outside `ipc.ts` (rule 1 below is about hand-
written `invoke()` calls, not about official Tauri plugin JS APIs).

---

## 10. Rules for implementers

1. **No `invoke` outside `src/lib/ipc.ts`.** Feature code imports typed wrappers.
2. **Every `Channel` subscription is torn down** in a `useEffect` cleanup; a leaked
   channel holds a Rust task alive.
3. **Commands never block.** Anything longer than ~50 ms spawns a task and streams.
4. **Errors are never stringified in Rust.** Return the typed `AppError`; the UI decides
   the wording, so a single error can render differently in a toast and in a panel.
5. **Payload size.** A single channel message stays under ~256 KB. Tool results larger
   than that are truncated with `full: None` and fetched on demand via a separate command.
6. **Backwards compatibility.** All event enums carry `#[serde(other)]`-style tolerance on
   the TS side: the reducer's `default` branch logs and ignores rather than throwing, so a
   newer Rust core never white-screens an older renderer during development.
