# ARCHITECTURE.md — Vibe Hardware

Technical design. Read after `SPEC.md`.

---

## 1. Stack

| Layer | Choice | Why |
|---|---|---|
| Desktop shell | **Tauri v2** | Small binaries, Rust core, per-window capabilities, mature updater |
| UI | **React 19 + TypeScript + Vite** | Stated requirement |
| State | **Zustand** slices + **TanStack Query** for CLI-backed reads | Streaming state is imperative and high-frequency; Redux ceremony is not worth it here |
| Styling | **Tailwind** + **Radix primitives** | Accessible primitives for free (NFR-A1) |
| Terminal rendering | **xterm.js** for the Logs and Monitor panes | Correct ANSI/SGR handling, virtualisation, selection — do not hand-roll |
| Diff view | **`diff` + a read-only CodeMirror 6 instance** | Read-only viewer is not "a code editor"; it is a diff renderer |
| Async runtime | **tokio** | Process and serial I/O |
| Process control | `tokio::process` + `nix`/`windows-sys` for process-group kills | Killing grandchildren matters (NFR-R2) |
| Serial | **`serialport`** crate (+ `tokio` bridge) | In-process monitor (FR-DEV-5) |
| INI | **format-preserving INI editor** (e.g. `rust-ini` with a custom writer, or `configparser` round-trip) | FR-INI-4 |
| Git | **`git2`** (libgit2) with a `git` CLI fallback | Snapshots without requiring a git install |
| Logging | `tracing` + `tauri-plugin-log` | NFR-R1 |

### Tauri plugins used

`dialog` (folder pickers), `fs` (workspace-scoped only), `store` (settings), `opener`
(open in editor / reveal), `updater`, `single-instance`, `log`, `os`, `process`,
`window-state`.

**Deliberately not used: `shell`.** Exposing a shell capability to the frontend would
put process spawning behind a JS-reachable permission surface. All spawning lives in
Rust behind typed commands (NFR-S3).

---

## 2. Layer diagram

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                          React Frontend (renderer)                           │
│  ┌──────────┬──────────┬──────────┬──────────┬──────────┬─────────────────┐  │
│  │  Chat    │  Logs    │ Changes  │ Problems │ Monitor  │ Project Settings│  │
│  └──────────┴──────────┴──────────┴──────────┴──────────┴─────────────────┘  │
│    zustand stores: session · pipeline · device · project · doctor · ini       │
└───────────────┬──────────────────────────────────────────┬───────────────────┘
      invoke()  │                                          │  Channel<T>
   (request/resp)│                                          │ (ordered streams)
┌───────────────▼──────────────────────────────────────────▼───────────────────┐
│                          Rust Core  (src-tauri)                              │
│                                                                              │
│  commands/        thin, typed Tauri command handlers                         │
│  ─────────────────────────────────────────────────────────────────────────   │
│  core/                                                                       │
│    ├─ toolchain/   Doctor probes, resolvers, installers                      │
│    ├─ claude/      turn runner, NDJSON parser, session store                 │
│    ├─ pio/         command builders, output parsers, option schema           │
│    ├─ project/     registry, create/open, CLAUDE.md generation               │
│    ├─ ini/         format-preserving read/patch/write + effective merge       │
│    ├─ device/      enumeration, PortBroker, monitor, telemetry adapters      │
│    ├─ snapshot/    shadow git, diff, revert                                  │
│    ├─ diag/        GCC/linker diagnostic parser, size parser                 │
│    └─ proc/        ProcessSupervisor: spawn, stream, signal, reap            │
└───────────────┬──────────────────────────────────┬───────────────┬───────────┘
                │                                  │               │
      ┌─────────▼─────────┐        ┌───────────────▼──────┐  ┌─────▼─────────┐
      │ PlatformIO Core   │        │   Claude Code CLI    │  │ Serial port   │
      │ pio project init  │        │ claude -p            │  │ (serialport   │
      │ pio run [-t ...]  │        │   --output-format    │  │  crate, direct)│
      │ pio device list   │        │     stream-json      │  └───────────────┘
      │ pio pkg ...       │        │   --resume <id>      │
      │ pio project config│        └──────────────────────┘
      │ pio pkg exec …    │
      └───────────────────┘                  │
                                   ┌─────────▼──────────┐
                                   │ PlatformIO Registry│
                                   │ api.registry…/v3   │
                                   └────────────────────┘
```

---

## 3. Module responsibilities

### `core/proc` — ProcessSupervisor

The single choke point for spawning anything. Nothing else in the codebase calls
`Command::new`.

```rust
pub struct SpawnSpec {
    pub program: PathBuf,        // resolved absolute path, never a bare name
    pub args: Vec<String>,       // argv array — never a shell string
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,   // additive; secrets injected here only
    pub kind: ProcKind,          // Pio | Claude | Installer | Tool
    pub label: String,           // shown in UI and logs
}

pub struct Handle {
    pub id: ProcId,
    pub started_at: Instant,
}

impl ProcessSupervisor {
    pub async fn spawn_streaming(&self, spec: SpawnSpec, sink: LineSink) -> Result<Handle>;
    pub async fn spawn_capture(&self, spec: SpawnSpec) -> Result<Output>;
    pub async fn interrupt(&self, id: ProcId) -> Result<()>;   // SIGINT / CTRL_BREAK
    pub async fn terminate(&self, id: ProcId) -> Result<()>;   // SIGTERM then SIGKILL
    pub fn running(&self) -> Vec<ProcSummary>;
}
```

Rules:
- Children are spawned into their own **process group** (Unix: `setsid`/`setpgid`;
  Windows: a **Job Object**), so `terminate` kills the whole tree. SCons spawns compilers
  as grandchildren and will otherwise survive (NFR-R2).
- `spawn_streaming` reads stdout and stderr on separate tasks, splits on `\n` **and**
  `\r` (progress bars use bare CR), and forwards lines tagged with their stream.
- All handles are registered in a table; `Drop` on the supervisor and a `RunEvent::Exit`
  hook terminate everything still alive.
- A per-`ProcKind` concurrency limiter: at most one `Pio` build-class process per
  workspace, at most one `Claude` turn per session.

### `core/toolchain` — Doctor

Each probe is an independent async fn returning `ProbeResult`. They run concurrently with
individual timeouts (default 5 s, 20 s for network). Doctor never blocks the UI.

Resolution order for a binary (FR-SETUP-6): explicit setting → `PATH` as seen by the
process → a hardcoded candidate list per OS → `which`/`where` executed through a login
shell as a last resort on macOS.

### `core/claude` — turn runner

```
run_turn(workspace, prompt, policy, session) →
    build argv  →  supervisor.spawn_streaming  →  NdjsonParser
                                                     │
                                       ┌─────────────┼───────────────┐
                                  ChatEvent      FileTouched     ResultMeta
                                  (Channel)      (snapshot diff)  (cost/turns)
```

The parser is a small state machine over newline-delimited JSON:

- Buffer bytes; split on `\n`; each complete line is `serde_json::from_str` into an
  untagged enum with a `#[serde(other)]` catch-all. **Malformed or unknown lines are
  logged and skipped, never fatal** (FR-CHAT-2).
- A single line can exceed typical buffer sizes (a large tool result). Use an unbounded
  line accumulator with a hard cap (e.g. 32 MB) that degrades to "output truncated".
- Text deltas are accumulated per content block and forwarded coalesced on a ~16 ms timer
  so React renders at frame rate rather than per token (NFR-P2).

### `core/device` — PortBroker

```rust
pub enum LeaseHolder { Monitor, Upload, Telemetry, External }

pub struct PortBroker { /* global, not per-window */ }

impl PortBroker {
    /// Acquires exclusively. If held by Monitor and `preempt` is set, stops the monitor,
    /// records that it should be restarted, and grants the lease.
    pub async fn acquire(&self, port: &str, who: LeaseHolder, preempt: bool)
        -> Result<Lease>;
}
```

`Lease` is an RAII guard. On drop, if a monitor was preempted and
`auto_reattach_monitor` is on, the broker restarts it with the same settings after a
short settle delay (boards re-enumerate after a flash; on many ESP boards the port
disappears and returns with the same path after ~500–1500 ms — the broker retries
re-open with backoff for up to 10 s before reporting failure).

### `core/snapshot` — the safety net

Prefer the workspace's own git repo. If absent, create `.vibe/shadow/` and operate with
an explicit git dir + work tree so `git status` in the user's terminal stays clean:

```
git --git-dir=<ws>/.vibe/shadow --work-tree=<ws> add -A
git --git-dir=<ws>/.vibe/shadow --work-tree=<ws> commit -m "pre-turn <turn_id>"
```

`.vibe/` is added to the shadow repo's own `info/exclude`. If the workspace *is* a git
repo, snapshots are stash-free: create a commit on a detached ref
(`refs/vibe/snapshots/<turn_id>`) using `git2`'s plumbing so the user's HEAD, index, and
branch are untouched.

### `core/ini`

Three operations, all format-preserving:

- `read_declared(path) -> IniDoc` — sections, keys, raw values, comments, spans.
- `patch(doc, edits) -> IniDoc` — surgical edits; unknown sections and comments survive.
- `effective(project_dir, env) -> Map<String, Value>` — shells out to
  `pio project config --json-output` and reshapes its nested-array output.

The option **schema** is fetched once per PIO version (see `CLI-CONTRACT.md` §6) and
cached under the app config dir keyed by `core_version`.

---

## 4. State machines

### 4.1 Pipeline (per workspace)

```
                 ┌──────────────── stop / error ───────────────┐
                 ▼                                             │
   ┌────────┐ prompt  ┌──────────┐ tool_use   ┌─────────┐ result ┌────────┐
   │  Idle  │────────▶│ Thinking │───────────▶│ Writing │───────▶│ Ready  │
   └────────┘         └──────────┘            └─────────┘        └───┬────┘
        ▲                                                           │ Build
        │                                                     ┌─────▼────┐
        │                                                     │ Building │
        │                                                     └─────┬────┘
        │                                         fail ◀────────────┤ ok
        │                                     ┌────────┐            ▼
        │                                     │ Failed │      ┌───────────┐
        │                                     └────┬───┘      │ BuildOk   │
        │                                          │ fix loop └─────┬─────┘
        └──────────────────────────────────────────┘                │ Upload
                                                              ┌─────▼──────┐
                                                              │ Uploading  │
                                                              └─────┬──────┘
                                                                    ▼
                                                              ┌────────────┐
                                                              │ Monitoring │
                                                              └────────────┘
```

Invariants:
- `Thinking`/`Writing` and `Building`/`Uploading` are mutually exclusive per workspace.
- In **Safe** policy, `BuildOk` is invalidated by any change to a watched file
  (`src/`, `include/`, `lib/`, `platformio.ini`); the Upload button disables with a
  tooltip explaining why.
- `Monitoring` is orthogonal in Fast-path/Watch policies — it can coexist with
  `Thinking` but never with `Uploading` (the broker enforces this).

### 4.2 Claude session

```
NoSession ──first turn (--session-id <uuid>)──▶ Active(id)
Active(id) ──turn (--resume id)──▶ Active(id)
Active(id) ──SIGTERM / exit 143──▶ Interrupted(id)   ; next --resume continues the turn
Active(id) ──"New session"──▶ NoSession (transcript archived)
Any ──auth probe fails──▶ Unauthenticated (turns blocked, banner shown)
```

### 4.3 Doctor

`Unknown → Probing → {Ready | NeedsSetup(list) | Broken(list)}`, with per-probe
sub-states and an `Installing(probe)` state that owns a log stream.

### 4.4 Device

`NoDevice → Detected(list) → Selected(port) → {Idle | Leased(holder)}`, plus
`Disappeared(port)` which retains the selection for re-binding (FR-DEV-2).

### 4.5 Monitor

`Stopped → Opening → Running → {Stopped | Preempted | Error(e)}`. `Preempted` is entered
only by the broker and carries the settings needed to resume.

---

## 5. Concurrency model

| Resource | Guard |
|---|---|
| Serial port | `PortBroker` lease (global across windows) |
| Workspace build | Per-workspace async mutex; a second Build request queues or is rejected with a toast |
| Claude session | Per-session mutex; the prompt box is disabled while held |
| `platformio.ini` | Read-modify-write under a file mutex + mtime check; if the file changed on disk since it was loaded, prompt to reload rather than clobber (Fay has it open in her editor) |
| Board catalogue / option schema | Read-through cache with single-flight |

---

## 6. Error model

One error enum surfaces to the frontend; every variant carries a stable `code`, a
human sentence, and an optional remediation action the UI can render as a button.

```rust
#[derive(Serialize)]
#[serde(tag = "code", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AppError {
    ToolMissing        { tool: String, install_action: bool },
    ToolTooOld         { tool: String, found: String, minimum: String },
    ClaudeUnauthenticated,
    ClaudePermissionDenied { denials: Vec<PermissionDenial> },
    ClaudeInterrupted  { session_id: String },
    NetworkUnavailable { host: String },
    PioCommandFailed   { argv: Vec<String>, exit_code: i32, tail: String },
    BuildFailed        { defects: usize },
    PortBusy           { port: String, held_by: String },
    PortDisappeared    { port: String },
    NotAPioProject     { path: String },
    IniParse           { path: String, line: Option<u32>, message: String },
    IniChangedOnDisk   { path: String },
    WorkspaceUntrusted { hooks: Vec<String>, mcp_servers: Vec<String> },
    Io                 { message: String },
}
```

Rules:
- `PioCommandFailed.tail` is the last ~4 KB of output, with any value matching a secret
  pattern redacted.
- The UI maps each `code` to a component; there is no generic "Something went wrong"
  dialog anywhere in the product.

---

## 7. Frontend structure

```
src/
  app/            router, window shell, theme, keybindings, command palette
  features/
    launcher/     recents, create wizard, open
    workspace/    two-pane layout, pipeline strip, sidebar cards
    chat/         message list, streaming renderer, tool-call cards, prompt deck
    logs/         xterm pane + log source multiplexer
    changes/      diff list, file diff viewer, revert actions
    problems/     defect list, source peek
    monitor/      xterm pane, controls, filter, send box
    settings-project/  ini Form/Raw, libraries, templates, CLAUDE.md editor
    settings-global/   toolchain, claude policy, pio settings, appearance
    doctor/       probe list, install flows
  lib/
    ipc.ts        typed invoke wrappers (generated types — see IPC-CONTRACT.md)
    events.ts     Channel subscription helpers with automatic cleanup
    ansi.ts       shared xterm instance factory
  stores/         zustand slices, one per state machine in §4
```

**Type generation.** Rust is the source of truth for every IPC type. Use `ts-rs` (or
`specta`) to emit `src/lib/bindings.ts` in a build step, and fail CI if the checked-in
bindings drift. Hand-written duplicates of Rust types are forbidden.

---

## 8. Streaming design

Two transports, chosen deliberately:

| Data | Transport | Why |
|---|---|---|
| Claude turn events, build/upload output, monitor lines, installer output | `tauri::ipc::Channel<T>` passed as a command argument | Ordered, typed, high-throughput; the documented mechanism for child-process output |
| Device list changes, doctor state changes, pipeline transitions, settings changes | Global events (`emit`) | Low frequency, fan-out to multiple windows |

Channel payloads are **coalesced in Rust**, not in JS: text deltas and log lines are
batched on a ~16 ms tick into arrays. One IPC message carrying 40 lines beats 40 messages
carrying one line each.

---

## 9. Security posture

1. **Capabilities.** `src-tauri/capabilities/default.json` grants only `dialog`,
   `opener` (scoped to file/dir URLs), `store`, `log`, `os`, `window-state`. No `shell`,
   no global `fs` (NFR-S3).
2. **Argv only.** Every external invocation is an argv array. There is no code path that
   builds a command string (NFR-S4).
3. **Path validation.** Workspace paths are canonicalised; all file operations assert the
   canonical path is a descendant of the workspace root or the app config dir.
4. **Workspace trust.** A workspace the app did not create is scanned for
   `.claude/settings.json` hooks, `.mcp.json`, and `.claude/agents/` before the first
   turn. Anything found is listed in a trust dialog. Until trusted, turns are blocked —
   because `claude -p` runs a project's hooks and connects its MCP servers with no prompt
   of its own (NFR-S5).
5. **Secrets.** A redaction filter sits in the log sink, matching
   `sk-ant-[A-Za-z0-9_-]+`, `ANTHROPIC_API_KEY=...`, and bearer tokens. The filter runs
   before anything reaches disk or the UI.
6. **Installer transparency.** Install actions display the exact command and its source
   URL, and require an explicit click (FR-SETUP-3).

---

## 10. Testing strategy

| Level | Scope | Tooling |
|---|---|---|
| Unit (Rust) | NDJSON parser (incl. malformed/partial/huge lines), diagnostic parser, size parser, INI round-trip (property test: parse→write is byte-identical for a corpus of real `platformio.ini` files), `pio project config` reshaping, port-lease FSM | `cargo test`, `proptest` |
| Unit (TS) | Store reducers, pipeline gating logic, ANSI handling | `vitest` |
| Golden-file | Captured real output from `pio run` (success, compile error, link error, size table), `pio device list --json-output`, `pio boards --json-output`, and a full `claude -p` NDJSON transcript | fixtures in `tests/fixtures/` |
| Integration | A `FakeCli` harness: a script that replays a golden transcript with realistic timing on stdout, substituted for the real binary via the resolved-path setting. Lets the whole pipeline be tested with no network, no hardware, and no cost | `cargo test` + `tauri::test` |
| E2E | Playwright against a dev build using `FakeCli` and a virtual serial pair (`socat -d -d pty,raw,echo=0 pty,raw,echo=0`) | Playwright |
| Manual matrix | Real hardware smoke test per release: ESP32-DevKitC, ESP8266 NodeMCU, Arduino Uno, RP2040 Pico, STM32 BluePill — on macOS, Windows, and Ubuntu | checklist in `ROADMAP.md` |

The `FakeCli` harness is not optional. Without it, nothing in this app is testable in CI.

---

## 11. Repository layout

```
vibe-hardware/
├─ CLAUDE.md
├─ docs/spec/*.md                # this pack
├─ package.json
├─ src/                          # React
├─ src-tauri/
│  ├─ Cargo.toml
│  ├─ tauri.conf.json
│  ├─ capabilities/default.json
│  └─ src/
│     ├─ main.rs
│     ├─ commands/{project,claude,pio,device,ini,doctor,settings}.rs
│     └─ core/{proc,toolchain,claude,pio,project,ini,device,snapshot,diag}/
├─ tests/fixtures/
└─ .github/workflows/{ci.yml,release.yml}
```
