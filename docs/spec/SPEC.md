# SPEC.md — Vibe Hardware, Product Specification v3

Status: implementation-ready
Supersedes: "Product Specifications: Hardware Vibe Coding Tool (v2)"

---

## 1. Product statement

Vibe Hardware is a desktop application for building embedded firmware by describing what
the device should do, rather than by typing code. The user picks a board, describes
behaviour in a chat deck, and the app drives Claude Code to write the firmware and
PlatformIO to compile, flash, and monitor it — without ever showing a code editor.

The app is a **shell around two CLIs it does not own**. Its value is in the parts those
CLIs do not provide: toolchain bootstrap, board telemetry, a safety net around
agent-written code, structured build diagnostics fed back into the conversation, serial
port arbitration, and a UI for `platformio.ini`.

### Non-goals (v1)

- A code editor, LSP, IntelliSense, or debugger UI.
- Schematic/PCB design. This is firmware only.
- Multi-board orchestration within one window.
- Cloud sync, accounts, collaboration, or telemetry upload.
- Replacing PlatformIO Home or the VS Code extension.

---

## 2. Gap analysis of the v2 spec

The v2 spec is a sound skeleton. Below is what stands between it and a shippable
product. Each gap becomes a requirement group in §5.

### 2.1 Blocking gaps — the app is unusable without these

| # | Gap in v2 | Why it blocks |
|---|---|---|
| G1 | **Assumes both CLIs are installed.** | On a clean machine neither `claude` nor `pio` exists. Requirement: a Doctor screen that detects, diagnoses, and installs both. → §5.1 |
| G2 | **Ignores Claude's permission model.** | `claude -p` starts in **Manual** permission mode on every plan. With no permission host, anything that would prompt is **denied**. A naïve `claude "prompt"` produces prose and writes *no files*. The app must pass an explicit permission mode and tool allowlist, or the core loop silently does nothing. → §5.3 |
| G3 | **Ignores Claude auth state.** | An unauthenticated CLI fails at *runtime* — `claude -p` prints the failure as the result on stdout with a non-zero exit, which is easy to render as "Claude said nothing". Auth must be a first-class, detectable state with a guided fix. → §5.1 |
| G4 | **"Claude writes changes silently to the local workspace" with no review, diff, or undo.** | This is the single largest risk in the product. An agent editing firmware for a physical device, unsupervised, with no revert, will eventually brick a board or destroy work. A change journal with snapshot/diff/revert is mandatory. → §5.4 |
| G5 | **No serial port arbitration.** | `pio device monitor` holds the port. `pio run -t upload` needs it. On most platforms the second one fails with a permission or busy error. Ports must be leased through a single owner. → §5.7 |
| G6 | **No cancellation.** | Builds take minutes; Claude turns take minutes. Without stop buttons and correct signal handling the UI is a hostage. → §5.3, §5.5 |
| G7 | **No `platformio.ini` UI**, though the user explicitly requires it. | → §5.6 |

### 2.2 Significant gaps — shippable but poor without these

| # | Gap | Resolution |
|---|---|---|
| G8 | Build failures are dumped as raw text. | Parse GCC/Clang diagnostics into structured defects; offer one-click **"Ask Claude to fix these N errors"**. → §5.5 |
| G9 | No session persistence. | Conversations vanish on restart. Persist transcripts and resume Claude sessions by id. → §5.3 |
| G10 | No library management. | `lib_deps` is how embedded work actually happens. Needs search + install + version pinning. → §5.6 |
| G11 | Telemetry is ESP-only (`esptool.py flash_id`). | Board-family telemetry adapters with graceful degradation. → §5.2 |
| G12 | No project manager. | Users have many projects. Needs a registry, recents, and open/close. → §5.2 |
| G13 | `pio device monitor` used as the in-app monitor. | It is an interactive terminal program with its own menu/exit-char handling. Spawning it to scrape stdout is fragile. Use the `serialport` Rust crate in-process; keep `pio device monitor` as an "open in external terminal" action. → §5.7 |
| G14 | No offline behaviour. | `pio project init -b <board>` needs the registry on first use and hangs then fails (observed: ~60 s then `HTTPClientError`). Needs pre-flight connectivity check and a clear error. → §5.1 |
| G15 | No cost/usage surface. | The `result` event carries `total_cost_usd`, `num_turns`, `duration_ms`. Show them. → §5.3 |
| G16 | No hot-plug handling. | Boards get unplugged mid-build. Poll and react. → §5.7 |
| G17 | No Linux udev / Windows driver guidance. | The #1 support issue in embedded tooling. → §5.1 |
| G18 | No firmware size tracking. | `pio run -t size` output is the main health metric for a constrained target. Track it across builds. → §5.5 |

### 2.3 Production-hygiene gaps

| # | Gap | Resolution |
|---|---|---|
| G19 | Packaging, signing, notarization, auto-update | → §7.4 |
| G20 | Structured logging, crash reporting, log export | → §7.3 |
| G21 | Secret hygiene — never log `ANTHROPIC_API_KEY`, OAuth tokens, or transcript contents to disk unencrypted without consent | → §7.5 |
| G22 | Accessibility (keyboard-only operation, contrast, screen reader labels on telemetry) | → §7.2 |
| G23 | First-run onboarding | → §5.1 |
| G24 | Crash/kill recovery — orphaned `pio`/`claude` child processes | → §7.3 |

---

## 3. Personas

- **Maker Mo.** Hobbyist. Has a blue pill ESP32 from a marketplace and no toolchain.
  Success = blinking LED in under 10 minutes from a cold machine, without a terminal.
- **Firmware Fay.** Professional embedded engineer. Has PlatformIO already. Uses the app
  to skip boilerplate for driver bring-up. Success = the app **adopts** her existing
  `~/.platformio`, never fights her `platformio.ini`, and always lets her open the real
  editor.
- **Educator Ed.** Runs a lab of 20 students. Success = deterministic setup, offline-
  tolerant, and a "reset this project" button.

---

## 4. Core concepts and vocabulary

| Term | Meaning |
|---|---|
| **Workspace** | A directory that is a PlatformIO project (contains `platformio.ini`) **and** is registered with the app. |
| **Environment (env)** | A `[env:*]` section in `platformio.ini`. One env is *active* at a time. |
| **Target board** | The `board` id of the active env. |
| **Device** | A physical serial device currently enumerated. Bound to the workspace by port path, best-effort by USB hwid. |
| **Turn** | One user prompt → one `claude -p` process → one result event. |
| **Session** | An ordered list of turns sharing a Claude `session_id`. One session per workspace by default. |
| **Snapshot** | A git commit in the workspace's shadow history taken immediately before a turn mutates files. |
| **Pipeline** | The state machine `Idle → Thinking → Writing → Ready → Building → Uploading → Monitoring`. |
| **Lease** | Exclusive claim on a serial port held by exactly one subsystem (monitor, upload, or telemetry probe). |

---

## 5. Functional requirements

Requirements are numbered `FR-<area>-<n>` and referenced from `ROADMAP.md`.

### 5.1 Toolchain, setup, and onboarding

> Full mechanics in [`TOOLCHAIN-SETUP.md`](./TOOLCHAIN-SETUP.md).

- **FR-SETUP-1** On launch the app runs **Doctor**, a set of independent probes, each
  resolving to `Ok { version, path } | Missing | Degraded { reason } | Error { detail }`:
  `claude` binary, Claude auth, `pio` binary, PlatformIO core dir writability, Python 3
  (only if PIO install is needed), network reachability of
  `api.registry.platformio.org`, serial permissions (Linux udev rules; macOS/Windows
  driver hints), and git availability.
- **FR-SETUP-2** Doctor results are cached with the probed version strings and re-run on
  demand, on app focus after >6 h, and after any install action.
- **FR-SETUP-3** If `claude` is missing, the Settings → Toolchain screen offers a
  one-click install that runs the official installer for the host OS, streams its output
  into a log pane, and re-probes on completion. The app **never** pipes a downloaded
  script into a shell without showing the user the exact command first.
- **FR-SETUP-4** If `claude` is present but unauthenticated, the app must not attempt a
  headless login. It surfaces an **Authenticate** action that opens an interactive
  `claude` session in the user's terminal (or offers `claude setup-token` for a
  long-lived token), then re-probes.
- **FR-SETUP-5** If `pio` is missing, the app offers: **(a)** adopt an existing install
  found at a known path, or **(b)** install via `get-platformio.py` into
  `~/.platformio/penv`. Option (b) requires a detected Python ≥ 3.6; if absent, the app
  states which Python to install and links to it, and does not attempt to install Python.
- **FR-SETUP-6** The resolved absolute paths to `pio` and `claude` are stored in global
  settings and used for every invocation. The app never relies on the GUI process
  inheriting a login shell `PATH` (this is the classic macOS `.app` failure mode) — it
  additionally probes `~/.local/bin`, `~/.platformio/penv/bin`, `/opt/homebrew/bin`,
  `/usr/local/bin`, and the Windows equivalents.
- **FR-SETUP-7** On Linux, if serial devices are not accessible, Doctor detects it and
  offers to install the PlatformIO udev rules, showing the exact privileged commands for
  the user to run. The app never invokes `sudo` itself.
- **FR-SETUP-8** First-run shows a 4-step onboarding: Toolchain → Authenticate → Create
  or open a project → Plug in a board. Each step is skippable and re-enterable.
- **FR-SETUP-9** All network-dependent actions pre-check connectivity and fail fast with
  "PlatformIO's registry is unreachable — you can still open existing projects and build
  with already-installed platforms" rather than hanging.

### 5.2 Project lifecycle

- **FR-PROJ-1 Create.** A wizard: (1) pick a board — searchable list built from
  `pio boards --json-output`, filterable by platform, framework, MCU, vendor, and
  connectivity, with Flash/RAM/frequency shown; (2) pick a framework offered by that
  board; (3) pick a parent directory via the native dialog and a project name;
  (4) options — sample code, git init, generate `CLAUDE.md`.
- **FR-PROJ-2** Creation runs `pio project init` with the chosen board and options,
  streaming progress. First-time use of a platform downloads a toolchain (hundreds of MB)
  — the UI must show this as a long-running, cancellable step with an explanation, not a
  frozen spinner.
- **FR-PROJ-3 Open.** Open any existing directory. If it has no `platformio.ini`, offer
  to initialize it. If it has one, parse it and populate the env picker.
- **FR-PROJ-4 Registry.** A persisted list of known projects with name, path, board,
  last-opened, and last-build-status. Missing paths are flagged, not silently dropped.
- **FR-PROJ-5 Board catalogue cache.** The full board list is cached on disk with a TTL
  and a "refresh" action, so the create wizard works offline after first use.
- **FR-PROJ-6 Open in editor.** An action that opens the workspace in the user's editor.
  Detection order: explicit setting → `$EDITOR`/`$VISUAL` → known editors on PATH
  (`code`, `cursor`, `zed`, `subl`, `idea`, `nvim`) → OS default handler for the folder.
  A "Reveal in Finder/Explorer" action sits beside it.
- **FR-PROJ-7 Environment switcher.** If `platformio.ini` has several `[env:*]` sections,
  a picker selects the active one; every build/upload/monitor/telemetry action is scoped
  to it via `-e`.
- **FR-PROJ-8 Context grounding.** On project creation the app writes a `CLAUDE.md` into
  the workspace containing: the board id and its brief data (MCU, F_CPU, flash, RAM,
  frameworks), the active framework, the contents of `platformio.ini`, the project
  layout convention, and house rules (e.g. "never add a library without adding it to
  `lib_deps`", "target is memory-constrained: avoid dynamic allocation in `loop()`").
  It is regenerated on board change and is user-editable via the Settings screen.

### 5.3 Conversational loop

- **FR-CHAT-1** A turn spawns exactly one `claude` process in the workspace directory
  with `-p`, `--output-format stream-json`, `--verbose`,
  `--include-partial-messages`, an explicit `--permission-mode`, an explicit
  `--allowedTools` set, and `--session-id` (first turn) or `--resume <id>` (subsequent).
  `--bare` is **never** used: it skips `CLAUDE.md` and refuses to read OAuth credentials.
- **FR-CHAT-2** The NDJSON stream is parsed incrementally. Unknown `type` / `subtype`
  values are ignored, not fatal — the CLI adds event kinds over time.
- **FR-CHAT-3** The chat renders, in order: streaming assistant text; a compact card per
  tool call (`Read src/main.cpp`, `Edit src/main.cpp`, `Bash pio run`) with an expandable
  result; `system/api_retry` as a non-alarming "retrying (attempt N)" chip; and a final
  turn footer showing duration, turn count, and `total_cost_usd`.
- **FR-CHAT-4 Permission policy.** Three user-selectable policies, defaulting to
  **Guarded**:
  - *Guarded* (default): `--permission-mode acceptEdits`, `--allowedTools` limited to
    `Read,Edit,Write,Glob,Grep` plus a narrow `Bash(pio *)` prefix rule. No arbitrary
    shell.
  - *Assisted*: Guarded plus `Bash` generally, and `--permission-mode auto`.
  - *Unrestricted*: `--dangerously-skip-permissions`. Gated behind a typed confirmation
    that names the workspace path, and is reset to Guarded on every app update.
  All policies pass `--permission-prompts none` so that a denied action fails fast with a
  visible `permission_denied` instead of hanging on a prompt nobody can answer. If the
  installed CLI predates that flag, the app omits it and relies on the mode alone.
- **FR-CHAT-5 Stop.** A Stop button sends `SIGINT` to end the turn cleanly, and escalates
  to `SIGTERM` after a grace period. `SIGTERM` leaves the turn unfinished (exit 143) and
  it resumes on the next `--resume`; the UI must state that plainly rather than showing a
  generic error. On Windows, use the job-object/CTRL_BREAK equivalent.
- **FR-CHAT-6 Session persistence.** Every turn is appended to a per-workspace session
  file: prompt, model, session id, all parsed events, result metadata, snapshot id, and
  the set of files touched. Reopening a project restores the visible history and the
  resumable session id.
- **FR-CHAT-7 New session.** An explicit action starts a fresh `session_id` while keeping
  the transcript archive.
- **FR-CHAT-8 Slash passthrough.** Text beginning with `/` is passed through to the CLI
  unchanged (skills and custom commands work in `-p`), except for a small set of
  app-owned commands (`/build`, `/upload`, `/monitor`, `/doctor`) which are intercepted.
- **FR-CHAT-9 Attachments.** The user may attach a datasheet PDF, a photo of wiring, or a
  captured serial log to a prompt. Attachments are copied into `.vibe/attachments/` and
  referenced by relative path in the prompt text so Claude's own `Read` tool fetches them.
- **FR-CHAT-10 Model and effort.** Model selection (`--model`) is exposed in settings with
  a default of `sonnet`, overridable per turn.
- **FR-CHAT-11 Failure surfacing.** A non-zero exit with no `result` event, a `result`
  with `is_error: true`, and a `permission_denials` array are three distinct, separately
  worded UI states. "Claude returned nothing" is never an acceptable message.

### 5.4 Change safety net *(new — closes G4)*

- **FR-SAFE-1** If the workspace is a git repo, the app takes a snapshot before each
  turn. If it is not, the app creates a **shadow repo** at
  `.vibe/shadow/` (a git dir with `--work-tree` pointed at the project) so the user's own
  VCS is never touched.
- **FR-SAFE-2** After a turn, the app computes the diff between snapshot and working tree
  and shows a **Changes** panel: files added/modified/deleted with line counts, and a
  read-only side-by-side diff per file with syntax highlighting.
- **FR-SAFE-3 Revert.** Per-file and whole-turn revert, both restoring from the snapshot.
  Revert is itself snapshotted so it can be undone.
- **FR-SAFE-4 Guard rails.** Before a turn, the app records the set of files under the
  workspace. After the turn it flags any write **outside** `src/`, `include/`, `lib/`,
  `test/`, `data/`, and `platformio.ini` as a warning banner; writes outside the
  workspace root are blocked by not passing `--add-dir` and are reported if attempted.
- **FR-SAFE-5** `platformio.ini` changes made by Claude are highlighted specially,
  because they can change the board, the upload protocol, or the flash layout. The
  Changes panel renders an ini-aware diff for that file.
- **FR-SAFE-6** A "Reset project to last known-good build" action restores the snapshot
  associated with the most recent successful `pio run`.

### 5.5 Build, upload, and diagnostics

- **FR-BUILD-1** `[Build]` runs `pio run -e <env> -d <dir>`, streaming combined
  stdout/stderr into the log pane with ANSI colour preserved (the app must **not** pass
  `--no-ansi` when it wants colour, and must render ANSI SGR sequences).
- **FR-BUILD-2** `[Upload]` runs `pio run -e <env> -t upload --upload-port <port>`, after
  acquiring the port lease (§5.7).
- **FR-BUILD-3 Pipeline policy** (global setting, three values):
  - *Safe* (default): Upload is disabled until a Build for the current file state has
    succeeded. Any file change invalidates it.
  - *Fast-path*: Upload implies Build; a single `pio run -t upload` invocation does both,
    and the UI shows compile and flash as two sub-steps of one run.
  - *Watch*: after a turn's changes land, Build runs automatically. Upload still manual.
- **FR-BUILD-4 Diagnostics.** stdout is parsed for `path:line:col: error|warning|note:
  message` and for the linker's `undefined reference` form. Parsed defects appear in a
  **Problems** list; clicking one reveals the source line in the read-only viewer and
  offers "Open in editor at this line".
- **FR-BUILD-5 Fix loop.** When a build fails, a prominent **"Ask Claude to fix these N
  errors"** button composes a prompt containing the structured defects plus the last ~100
  lines of build output and submits it as a turn.
- **FR-BUILD-6 Size tracking.** After a successful build, the RAM/Flash usage lines from
  `pio run` are parsed and stored per build; the telemetry card shows current usage as
  bars plus the delta against the previous successful build.
- **FR-BUILD-7 Extra targets.** A secondary menu exposes `clean`, `fullclean`, `size`,
  `erase`, `uploadfs`/`buildfs` (when the platform provides them), and `compiledb`.
  Available targets are read from `pio run --list-targets` (text output — there is no
  JSON form) and cached per environment. The list is only obtainable once the platform is
  installed; before that, the menu shows the universal subset.
- **FR-BUILD-8 Cancellation.** Stop kills the `pio` process tree, not just the parent —
  SCons spawns compilers as grandchildren.
- **FR-BUILD-9 Static analysis.** `pio check --json-output` is exposed as an optional
  action feeding the same Problems list.
- **FR-BUILD-10 Unit tests.** `pio test --json-output` is exposed for projects with a
  `test/` directory, rendered as a pass/fail list.

### 5.6 `platformio.ini` and package management *(new — closes G7, G10)*

- **FR-INI-1 Dual-mode editor.** The Project Settings screen has two tabs over the same
  file: **Form** and **Raw**. They stay in sync; switching tabs commits pending edits.
- **FR-INI-2 Schema-driven form.** The form is generated from PlatformIO's own option
  registry, not hardcoded. The app extracts it once per PIO version by executing a
  one-line Python snippet in the PIO environment that serialises
  `platformio.project.options.ProjectOptions` (see `CLI-CONTRACT.md` §6). This yields
  84 options across the groups `generic, directory` (scope `platformio`) and
  `platform, build, upload, monitor, library, check, test, debug, advanced` (scope `env`),
  each carrying `name`, `description`, `type` (`string|integer|choice|path|...`),
  `multiple`, `default`, and `choices`/`min`/`max` where applicable.
  Widgets map from `type` × `multiple`:
  | type | multiple | widget |
  |---|---|---|
  | `string` | false | text input |
  | `string` | true | editable string list (one value per line, matching INI multi-line syntax) |
  | `integer` | false | number input honouring `min`/`max` |
  | `choice` | false | select |
  | `choice` | true | multi-select chip group |
  | `path`/`directory` | false | text input + folder picker |
  | `boolean` | false | switch |
  Each field shows PlatformIO's own `description` as help text, its default as the
  placeholder, and a badge when the value is inherited from `[env]` or via `extends`.
- **FR-INI-3 Effective vs declared.** The form shows *declared* values for the selected
  section, and an "Effective" column populated from `pio project config --json-output`,
  which resolves inheritance. Both must be visible; a user who sets `monitor_speed` in
  `[env]` needs to see why `[env:esp32dev]` shows 115200.
- **FR-INI-4 Round-trip fidelity.** Writing the file must preserve comments, ordering,
  blank lines, and sections the app does not understand. Parse and re-emit with a
  format-preserving INI editor; never regenerate the file from a model.
- **FR-INI-5 Validation.** On save, run `pio project config --lint`. **Note:** with
  `--json-output` this command emits a Python `repr`, not JSON (single quotes) — see
  `CLI-CONTRACT.md` §4.3. Parse the human-readable form or the repr defensively; do not
  feed it to `JSON.parse`.
- **FR-INI-6 Global settings UI.** A separate screen wraps `pio settings get/set` for the
  global PlatformIO settings (`enable_telemetry`, `enable_cache`, `projects_dir`,
  `force_verbose`, `check_platformio_interval`, `check_prune_system_threshold`,
  `disable_udev_rules_check`, `enable_proxy_strict_ssl`), each rendered from the
  name/value/description triple that `pio settings get` prints, with a per-setting
  "reset to default" action.
- **FR-INI-7 Library search.** A library browser searches the PlatformIO registry.
  `pio pkg search` has **no** JSON output in Core 6.2.0, so the app calls the registry
  HTTP API directly: `GET https://api.registry.platformio.org/v3/search?query=...`
  with `page` and `sort` (`relevance|popularity|trending|added|updated`), falling back to
  the mirror `api.registry.nm1.platformio.org`. Qualifiers (`keyword:`, `framework:`,
  `platform:`, `header:`, `owner:`) are exposed as filter chips.
- **FR-INI-8 Library install.** Installing runs
  `pio pkg install -d <dir> -e <env> -l "<owner>/<name>@^<version>"`, which writes
  `lib_deps` for you. A version selector offers exact / caret / tilde pinning. Uninstall
  uses `pio pkg uninstall` with the same shape.
- **FR-INI-9 Installed packages.** The Libraries tab lists installed platforms, tools,
  and libraries for the active env (`pio pkg list`, text output, parsed) with an
  "outdated" badge from `pio pkg outdated`.
- **FR-INI-10 Platform/tool management.** Advanced users can install a specific platform
  or tool version (`pio pkg install -p`, `-t`) and prune caches (`pio system prune`).
- **FR-INI-11 Templates.** Saving the current `platformio.ini` as a named template, and
  applying a template to a new project, is a first-class action.

### 5.7 Device, telemetry, and the serial monitor

- **FR-DEV-1 Enumeration.** `pio device list --json-output` returns a flat array of
  `{ port, description, hwid }` when only serial devices are requested. Poll every 2 s
  while the app is focused, 10 s in the background, and immediately after any upload.
- **FR-DEV-2 Selection and stickiness.** The user selects a port; the app remembers the
  USB `VID:PID` + serial number from `hwid` and re-binds automatically when the same
  device reappears on a different port path.
- **FR-DEV-3 Telemetry adapters.** Board-family adapters, each optional and
  failure-tolerant:
  - *ESP32/ESP8266*: `pio pkg exec -p "tool-esptoolpy" -- esptool.py flash_id` for chip
    type, flash vendor, flash size, and MAC. The entrypoint name changed across esptool
    versions (`esptool.py` vs `esptool`); the adapter probes both and caches which worked.
  - *Generic*: port path, USB description, `hwid`, and the board's own brief data from
    the catalogue (MCU, F_CPU, max flash, max RAM).
  A missing or failing adapter renders "not available for this board" — never an error
  dialog, and never a blocked UI.
- **FR-DEV-4 Port leases.** A single `PortBroker` in the Rust core owns every serial
  port. Monitor, upload, and telemetry probe each acquire an exclusive lease. If Upload
  is pressed while the monitor holds the port, the broker (a) stops the monitor,
  (b) performs the upload, (c) restarts the monitor with the same settings, and the UI
  narrates each step. This behaviour is a setting (`auto_reattach_monitor`, default on).
- **FR-DEV-5 In-app monitor.** The monitor reads the port directly through the
  `serialport` crate, **not** by scraping `pio device monitor`. Settings come from the
  active env's `monitor_*` options (`monitor_speed`, `monitor_parity`, `monitor_rts`,
  `monitor_dtr`, `monitor_eol`, `monitor_echo`, `monitor_encoding`, `monitor_raw`) so the
  in-app monitor and the CLI behave identically. Features: ANSI rendering, timestamps,
  autoscroll toggle, regex filter, a line-count-capped ring buffer, "clear", "save log to
  file", and a send box honouring the EOL setting.
- **FR-DEV-6 External monitor.** An "Open in terminal" action launches
  `pio device monitor -d <dir> -e <env>` in the platform's terminal, for users who want
  the real thing. The lease is released first.
- **FR-DEV-7 Log → Claude.** Selecting lines in the monitor offers "Send to Claude",
  which inserts them into the prompt box as a fenced block. This closes the debugging
  loop that makes the product worth using.
- **FR-DEV-8 Hot-plug.** Disconnection during build/upload/monitor produces a specific,
  actionable message and leaves the pipeline in a recoverable state.
- **FR-DEV-9 Auto-detect on connect.** When a new device appears and its `hwid` matches a
  known USB-serial bridge (CH340, CP210x, FTDI, or a native USB CDC id), the app offers a
  toast: "New board detected on <port> — use it?".

### 5.8 Application shell

- **FR-UI-1** Two-pane layout: sidebar ~25 % (min 280 px, resizable, collapsible), main
  canvas ~75 %.
- **FR-UI-2** Sidebar, top to bottom: Project card (name, path, active env picker, board
  id, Claude session health) → Hardware telemetry card → Firmware size card → Control
  deck (`Build`, `Upload`, `Monitor`, plus an overflow menu of extra targets) → Problems
  badge → Doctor status chip.
- **FR-UI-3** Main canvas is tabbed: **Chat** (default), **Logs**, **Changes**,
  **Problems**, **Monitor**, **Project Settings**.
- **FR-UI-4** A persistent pipeline strip sits above the canvas showing the state machine
  with the current step highlighted, elapsed time, and a Stop button when anything runs.
- **FR-UI-5** The prompt deck is multi-line, submits on `Cmd/Ctrl+Enter`, keeps a
  per-project prompt history navigable with `Up`/`Down` when empty, and is disabled with
  an explanatory tooltip while a turn is in flight.
- **FR-UI-6** Every long-running action is cancellable and shows elapsed time. No
  indeterminate spinner may run for more than 2 s without accompanying text saying what
  is happening.
- **FR-UI-7** Light and dark themes, following the OS by default.
- **FR-UI-8** Global keyboard shortcuts: `Cmd/Ctrl+B` build, `Cmd/Ctrl+U` upload,
  `Cmd/Ctrl+M` monitor, `Cmd/Ctrl+K` command palette, `Cmd/Ctrl+,` settings,
  `Esc` stop.
- **FR-UI-9** Multiple windows, one workspace each. The Rust core is shared; the
  `PortBroker` is global across windows so two windows cannot fight over one board.

---

## 6. Screen inventory

| Screen | Purpose | Key requirements |
|---|---|---|
| **Launcher** | Recents, Create, Open, Doctor status | FR-PROJ-4, FR-SETUP-1 |
| **Onboarding** | 4-step first run | FR-SETUP-8 |
| **Create Project** | Board picker → framework → location → options | FR-PROJ-1/2 |
| **Workspace** | The two-pane main screen | FR-UI-1..9 |
| → Chat tab | Conversation, streaming, tool cards | FR-CHAT-* |
| → Logs tab | ANSI terminal block for pio/claude/installer output | FR-BUILD-1 |
| → Changes tab | Snapshot diff, per-file revert | FR-SAFE-* |
| → Problems tab | Structured defects from build/check | FR-BUILD-4/9 |
| → Monitor tab | In-app serial terminal | FR-DEV-5/7 |
| → Project Settings | `platformio.ini` Form/Raw, Libraries, Templates, CLAUDE.md | FR-INI-1..11, FR-PROJ-8 |
| **Global Settings** | Toolchain, Claude policy & model, PlatformIO global settings, Appearance, Editor, Advanced | FR-SETUP-*, FR-CHAT-4/10, FR-INI-6 |
| **Doctor** | Probe results, install actions, log export | FR-SETUP-1..7 |

---

## 7. Non-functional requirements

### 7.1 Performance
- **NFR-P1** Cold start to interactive shell < 1.5 s; Doctor probes run concurrently in
  the background and never block first paint.
- **NFR-P2** Streaming chat renders at ≥ 30 fps with a 200 KB assistant response; text
  deltas are batched per animation frame, not per event.
- **NFR-P3** The log pane virtualises and caps at a configurable 50 000 lines; the serial
  monitor ring buffer is capped by line count and total bytes.
- **NFR-P4** Board catalogue (~1 500 entries) filters in < 50 ms — index it once on load.

### 7.2 Accessibility & i18n
- **NFR-A1** Every action reachable by keyboard; visible focus rings; no
  hover-only affordances.
- **NFR-A2** Telemetry values exposed as text, not only as gauges; status conveyed by
  icon + text, never colour alone. WCAG 2.1 AA contrast in both themes.
- **NFR-A3** All user-facing strings in one resource module from day one, even though v1
  ships English only.

### 7.3 Reliability & observability
- **NFR-R1** Structured logs (`tauri-plugin-log` + `tracing`) to a rotating file, with an
  "Export diagnostics bundle" action producing a zip of logs, Doctor output,
  `pio system info --json-output`, `claude doctor` output, and the sanitised
  `platformio.ini`.
- **NFR-R2** Every spawned child is registered in a process table and killed on app exit
  and on panic, including grandchildren. No orphaned `pio`/compiler processes.
- **NFR-R3** Crash-safe persistence: session and settings writes are atomic
  (write-temp + rename).
- **NFR-R4** The app functions offline for: opening projects, building with installed
  platforms, monitoring, and browsing cached board data. It degrades, with explanation,
  for: project creation with a new platform, library search/install, and Claude turns.

### 7.4 Packaging & distribution
- **NFR-D1** Targets: macOS (universal `.dmg`, signed + notarized), Windows
  (`.msi`/NSIS, Authenticode-signed), Linux (`.AppImage` and `.deb`).
- **NFR-D2** Tauri updater with a signed update manifest; update checks are opt-out.
- **NFR-D3** Reproducible CI build; version and git SHA surfaced in About and in the
  diagnostics bundle.

### 7.5 Security & privacy
- **NFR-S1** The app stores no API keys by default. If the user supplies
  `ANTHROPIC_API_KEY`, it lives in the OS keychain, is injected into the child
  environment only, and is redacted from all logs.
- **NFR-S2** Transcripts are local-only. No analytics leave the machine in v1. A
  first-run notice states plainly that prompts, and the code Claude reads, are sent to
  Anthropic by the Claude CLI.
- **NFR-S3** Tauri capabilities are least-privilege: the frontend gets **no** direct
  `shell` or unrestricted `fs` capability. All process spawning happens in Rust behind
  typed commands; all filesystem access is scoped to the active workspace plus the app's
  own config dir.
- **NFR-S4** Arguments are always passed as argv arrays. No string interpolation into a
  shell. Any path, port, or env name that reaches a command is validated against an
  allowlist pattern first.
- **NFR-S5** A workspace's `.claude/settings.json`, `.mcp.json`, and hooks execute when
  `claude -p` runs there. Opening a project from an untrusted source is therefore a code
  execution risk. The app shows a trust prompt the first time a workspace it did not
  create is opened, naming any hooks or MCP servers it found.

### 7.6 Compatibility
- **NFR-C1** Minimum PlatformIO Core **6.1.0**; verified against **6.2.0**. The app reads
  `pio system info --json-output` → `core_version` and warns below minimum.
- **NFR-C2** Minimum Claude Code **2.1.0**; features gated on version where the docs
  state a floor (e.g. `--permission-prompts` needs 2.1.259+). Feature-detect via the
  `capabilities` array on `system/init` where available, never by string comparison alone.
- **NFR-C3** OS floors: macOS 13+, Windows 10 1809+, Ubuntu 20.04+/Debian 10+.

---

## 8. Open questions for the product owner

1. Should a workspace be allowed to have more than one Claude session (e.g. one per
   feature branch), or is one-per-workspace correct forever?
2. Is a **"dry run"** mode wanted — Claude proposes a diff that the user approves before
   anything is written? This is stricter than FR-SAFE-2 and would use
   `--permission-mode plan` for the first pass.
3. Should the app ship curated **project templates** (blink, WiFi provisioning, MQTT
   sensor, BLE beacon) in v1, or only in a later release?
4. Does the ESP-focused telemetry justify bundling `esptool` independently of PlatformIO
   so telemetry works before any platform is installed?
5. `AppError::ClaudePermissionDenied.denials: Vec<PermissionDenial>` (`ARCHITECTURE.md`
   §6) is referenced but `PermissionDenial`'s fields are never defined anywhere in the
   pack. M0 defines it as `{ tool: String, reason: String }`, mirroring
   `ChatEvent::PermissionDenied`'s payload (`IPC-CONTRACT.md` §4) — revisit if a richer
   shape (e.g. the denied rule pattern) turns out to be needed once M4 is implemented.
6. `IPC-CONTRACT.md` §3's project command table has no way to cancel a `project_create`
   in progress, even though `ROADMAP.md` M2's own acceptance test requires "a working
   Cancel" on the toolchain download. M2 adds `project_cancel_create(proc_id) -> ()`,
   which just calls `ProcessSupervisor::terminate` on the real `ProcId` `project_create`
   already returns — matching `pipeline_stop`'s shape from §5 one milestone early. If M5
   ends up with a generic `proc_stop`, this should probably be renamed/merged into it.
7. Same section: nothing lists a workspace's `platformio.ini` environment names for the
   picker `FR-PROJ-7` requires, and the full parser (`core::ini`, `IniDocument`) isn't
   built until M7. M2 adds `project_list_envs(id) -> Vec<String>`, backed by a minimal
   `[env:*]`-section-name scanner (`core::project::env`) — not the format-preserving
   editor. Worth revisiting once `ini_read` exists in case it should absorb this instead.
8. `ARCHITECTURE.md` §6's `AppError` enum has no variant for "the claude process exited
   without ever producing a parseable `result` line and wasn't a clean SIGTERM/exit-143
   interruption" (that case is `ClaudeInterrupted`). M3 adds
   `ClaudeProcessFailed { exit_code, tail }`, mirroring `PioCommandFailed`'s shape
   (redacted last ~4 KB of stderr) so `ChatEvent::Failed` (`IPC-CONTRACT.md` §4) always has
   something typed to carry — never a stringified fallback (`FR-CHAT-11`).
9. `CLI-CONTRACT.md` §1.5 documents `text_delta`/`input_json_delta` as "the deltas the app
   cares about" but doesn't name the thinking-block delta shape, even though
   `ChatEvent::ThinkingDelta` (`IPC-CONTRACT.md` §4) already exists. M3's NDJSON parser
   assumes the standard Anthropic Messages API shape — `content_block.type == "thinking"`,
   `delta.type == "thinking_delta"`, `delta.thinking` — by analogy with the verified
   `text_delta`/`delta.text` pair. Unverified against a live transcript; revisit once one
   is captured.
10. `system/permission_denied`'s payload shape is undocumented beyond "emitted when a call
    is denied" (`CLI-CONTRACT.md` §1.5). M3's parser reads `tool`/`reason` defensively
    (falling back to `"unknown"`/`""`), matching the same assumed shape open question 5
    already flagged for `PermissionDenial`.
11. `ChatEvent::SubagentMessage` (`IPC-CONTRACT.md` §4) is a flat `{ role, text }` per
    message, but a subagent's `assistant`/`user` NDJSON line can carry a content array with
    several blocks (text, tool_use, tool_result) like the main conversation does. M3
    flattens each block to a short text summary (e.g. `Tool: Name({...})` for a `tool_use`
    block) rather than emitting one `SubagentMessage` per block-with-structure. Revisit if
    subagent tool calls need their own cards.
12. `ARCHITECTURE.md` §6's `AppError` enum has no variant for the snapshot/revert engine
    (`core::snapshot`, `FR-SAFE-1..6`) failing — a `git2::Error` (corrupt shadow repo,
    permission-denied write, etc.). M4 adds `SnapshotFailed { message }`, following the
    same pattern as `ClaudeProcessFailed` (open question 8): a typed variant rather than
    folding it into generic `Io`, since the UI likely wants to say "couldn't create a
    safety snapshot" specifically.
13. `FR-SAFE-6` ("Reset project to last known-good build") reads `.vibe/builds.json`'s
    `lastGoodSnapshot` (`DATA-MODEL.md` §7), but `builds.json` is written by the build
    pipeline, which doesn't exist until M5 — a forward dependency the milestone ordering
    doesn't call out. M4 implements `changes_reset_to_last_good_build` fully (reads the
    file, reverts to the recorded snapshot) but it's inert — returns `AppError::Io` with a
    clear "no successful build recorded yet" message — until M5 starts writing that file.
14. `FR-SAFE-2` says the Changes panel diffs "snapshot and working tree," not two
    snapshots. M4 takes this literally: `changes_for_turn`/`changes_diff` always diff a
    turn's `snapshotBefore` (`DATA-MODEL.md` §6) against the *current* on-disk working
    tree, not a second "after" snapshot (which the data model doesn't have a field for
    either). This is exact for the most recent turn; if further turns land afterward,
    revisiting an older turn's Changes tab shows cumulative changes since that turn's
    snapshot, not just that turn's own — a known staleness tradeoff, not a bug. Revisit if
    per-turn attribution across multiple later turns turns out to matter.
15. `FR-CHAT-4`'s "gated behind a typed confirmation that names the workspace path" implies
    the Unrestricted toggle is reached from *within* a workspace, but permission policy is
    a **global** setting (`DATA-MODEL.md` §3 `claude.permissionPolicy`) — there's no
    concept of "the workspace" on a pure global-settings screen, and `IPC-CONTRACT.md` §9
    has no `settings_set_project` command to make it a true per-project override either
    (`ProjectClaudeSettings.permissionPolicy` exists in the data model but nothing writes
    it). M4 mounts the policy selector in the Chat header specifically so the confirmation
    has a concrete path to show/match, but it still edits the *global* setting — choosing
    Unrestricted from one workspace makes it the default for every workspace until changed
    back. Revisit once/if a real per-project override is added.
16. `CLI-CONTRACT.md` §5.1's linker diagnostic form ("the linker's `undefined reference`
    form... plus the preceding `file:line:` line") is unverified against a real captured
    link failure — `tests/fixtures/pio-run-fail.txt` only has compiler (syntax) errors.
    M5's parser handles the shape a real GNU-ld undefined-reference failure is known to
    take (`file:line:` and `undefined reference to '...'` combined on one line) and, as a
    fallback, a bare `file:line:` line followed by an `undefined reference` with no prefix
    of its own. Revisit once a real linker failure is captured.
17. `FR-BUILD-2`'s upload needs "the port lease (§5.7)" — `PortBroker` (RAII leases, monitor
    preemption, hot-plug tracking) — but that's M6's `core/device` module, which doesn't
    exist yet. M5 adds a minimal, stateless `device_list_ports()` (wrapping
    `pio device list --serial --json-output`, already documented in `CLI-CONTRACT.md` §3.2)
    so Upload has *something* to pick a port from, and lets `--upload-port` be omitted
    entirely (PlatformIO auto-detects when it's absent). No leasing, no preemption, no
    re-enumeration-after-flash retry — those stay M6's job. Revisit once `PortBroker`
    exists; this should probably be absorbed into it rather than living standalone.
18. `ARCHITECTURE.md` §4.1's pipeline state machine spans both the chat turn lifecycle
    (`Idle`/`Thinking`/`Writing`) and the build lifecycle (`Building`/`BuildOk`/
    `Uploading`/`Monitoring`). M5 implements and emits `pipeline://state` for only the
    build half (`Idle` here meaning "no build/upload running," not literally the chat
    state) — wiring `Thinking`/`Writing` transitions from `claude_send_turn`'s own event
    stream is deferred. The chat UI already shows its own live/running state inline
    (`M3`), so this doesn't block FR-UI-4's build-facing pipeline strip; revisit if a
    single unified strip covering both halves turns out to be wanted.
19. `ARCHITECTURE.md` §6's `AppError` has nothing for "Upload was attempted but Safe
    policy's `BuildOk` isn't current" (`FR-BUILD-3`) — distinct from `BuildFailed`, which
    is about a build that actually ran and produced defects. M5 adds
    `UploadBlocked { reason }` so the UI can render a specific disabled-button tooltip
    rather than a generic error.
20. `ARCHITECTURE.md` §4.1's staleness rule ("Safe policy... any file change invalidates
    [`BuildOk`]") doesn't specify a detection mechanism, and this product has no code
    editor to hook edit events from (`CLAUDE.md`) — file changes come from Claude turns or
    an external editor either way. M5 polls: the max mtime under `src/`, `include/`,
    `lib/`, `platformio.ini` is recorded when `BuildOk` is entered and re-checked on demand
    (serving `pipeline_state`, before an Upload) rather than via a filesystem watcher.
    Cheap for a typical embedded source tree; revisit if a `notify`-based watcher turns out
    to be needed (e.g. for a live "stale" badge without the user taking an action first).
21. `IPC-CONTRACT.md` §6 has `device_list() -> Vec<SerialDevice>` but nothing to persist
    which port the user picked, and `pipeline_upload` needs one to pass as
    `--upload-port` (or omit, letting PlatformIO auto-detect — open question 17). M5 adds
    `device_set_preferred_port(workspace, port)`, writing
    `ProjectSettings.device.preferredPort` (already in `DATA-MODEL.md` §4, unused until
    now). Revisit once `PortBroker` exists in case device selection should route through
    it instead of a bare settings write.
22. `ARCHITECTURE.md` §3 sketches `PortBroker::acquire` returning an RAII `Lease`, which
    reads as though preemption (`FR-DEV-4`: stop the monitor, upload, restart the monitor)
    should fall out of `Drop` alone. `Drop` is synchronous and can't `.await` stopping a
    thread or restarting a monitor session, so M6 splits it: `PortBroker` itself only
    tracks *who* holds a port and grants a lease unconditionally when `preempt: true`; the
    actual "stop, do the thing, restart" sequence is orchestrated one layer up, in
    `commands::monitor::MonitorState::preempt_for_upload` and
    `commands::pipeline::run_build_or_upload`. Revisit if a future port user needs the same
    preempt-and-resume shape and this orchestration should be factored out of `pipeline.rs`.
23. **Resolved**, verified against real hardware. `CLI-CONTRACT.md` §7.5's documented ESP
    telemetry invocation (`pio pkg exec -p "tool-esptoolpy" -- esptool.py flash_id`) has no
    `--port` as written — tried against a real ESP32-C6-DevKitM-1 with several unrelated
    Bluetooth COM ports also present, omitting `--port` made esptool probe *every* serial
    port in turn (each with its own connect timeout), taking far longer and producing noisy
    interleaved errors before eventually reaching the right one by elimination. `--port` is
    esptool's own flag, not a `pio` one — everything after `pio pkg exec`'s `--` is
    forwarded to the sub-command verbatim, so passing it isn't inventing a `pio` flag
    (`CLAUDE.md` hard rule 1 is about `pio`/`claude` flags specifically). `core::device::telemetry`
    now passes `--port <the selected device's port>` explicitly.
24. **Resolved**, verified against real hardware. `esptool.py flash_id`'s own output format
    (`Manufacturer:`/`Detected flash size:` lines) is now captured in
    `tests/fixtures/esptool-flash-id-real.txt`, from the same real ESP32-C6-DevKitM-1 run
    that resolved open question 23 — `Manufacturer: c8` / `Detected flash size: 8MB`, which
    matched the parser's regexes without changes. That run also surfaced two esptool v5.3.0
    deprecation warning lines (`'esptool.py' is deprecated`, `'flash_id' is deprecated`)
    between the package-prefix line and the connect banner, which the fixture now covers —
    the parser already tolerated them since its regexes search anywhere in the text.
25. `pio device monitor`'s `--parity` accepts `N|E|O|S|M` (`CLI-CONTRACT.md` §3.3), but the
    `serialport` crate's `Parity` enum only has `None`/`Odd`/`Even` — no Space or Mark.
    `core::device::monitor::settings_from_ini` falls back to `None` for `S`/`M`. Revisit if
    a board that actually needs space/mark parity shows up (rare outside older RS-232 gear).
26. The in-app serial monitor (`core::device::monitor`) was implemented and unit-tested
    (settings parsing, EOL mapping, ring buffer capping) but not exercised against real
    hardware in this environment, unlike M5's Build/Upload path. Baud/parity/RTS/DTR
    handling and the read/write thread's actual behavior against a physical board should be
    verified before this ships — see the acceptance-test notes for this milestone.
27. `NFR-P3` specifies the serial monitor's ring buffer is "capped by line count and total
    bytes" but gives no figures (only the log pane's 50,000-line default is stated
    elsewhere). M6 picks `DEFAULT_MAX_LINES = 10_000` / `DEFAULT_MAX_BYTES = 5 MiB` as a
    starting point — unverified against any product decision. Revisit if these should be
    user-configurable via `GlobalSettings` the way `logs.maxLines` already is.
28. `device_telemetry`'s `refresh: bool` parameter (`IPC-CONTRACT.md` §6) mirrors
    `pipeline_targets`/`boards_list`'s cache-bypass convention, but telemetry is never
    cached in the first place (it always probes live hardware), so `refresh` currently has
    no effect. Kept for signature parity with the documented command; revisit if telemetry
    ever gains a cache worth bypassing.
29. `FR-DEV-6`'s "Open in terminal" needs a platform mechanism to launch a visible terminal
    running `pio device monitor -d … -e …`, which `CLI-CONTRACT.md` documents the invocation
    for but not the terminal-launch mechanism itself (that's OS-level, not a `pio`/`claude`
    flag). M6 implements this for Windows only (`cmd.exe /c start "" …`), since this dev
    environment is Windows-only and macOS/Linux's equivalents (`open -a Terminal`,
    `x-terminal-emulator`, etc.) aren't verifiable here. `monitor_open_external` returns a
    clear `AppError::Io` on other platforms rather than guessing. Revisit once macOS/Linux
    can be tested.
30. `DATA-MODEL.md` §8.1's multi-value continuation example shows `build_flags =` with no
    trailing space before the newline. A real `pio pkg install` run this session (writing
    `lib_deps` with two entries) showed `lib_deps = ` **with** a trailing space. `core::ini`'s
    own writer (`patch::format_entry_lines`) matches the real capture, not the doc example —
    verified byte-identical against `pio`'s own output in
    `core::ini::patch::tests::the_real_fixture_survives_an_edit_and_a_revert_byte_identical`.
31. `core::ini::document::resolve_inherited_from`'s `extends` handling only follows one
    level — if `env:b extends env:a` and `env:a extends env:c`, an option `env:b` inherits
    from `env:c` (two hops away) resolves to `inherited_from: None` rather than naming
    `env:c`, since the effective JSON has no origin-section field to cross-check against
    beyond one hop. Revisit if multi-level `extends` chains turn out to be common.
32. `CLI-CONTRACT.md` §2.3 documents 8 global `pio settings` names; a real
    `pio settings get` capture against this session's installed Core only returned 6 —
    `enable_telemetry` and `disable_udev_rules_check` both returned an empty table when
    queried by name individually (possibly removed/platform-gated in this Core version).
    `core::pio::global_settings` parses whatever the table actually contains rather than
    assuming the documented 8, which sidesteps the discrepancy but doesn't explain it.
    Separately, every row in that same capture was already at its default, so whether/how a
    *non-default* row's `[default]` bracket is actually formatted remains unverified.
33. `ini_template_apply`'s IPC signature (`workspace, name -> IniDocument`) writes
    immediately, but `DATA-MODEL.md` §10 says applying a template should show "the resulting
    diff for confirmation before writing." There's no separate non-mutating preview command
    in the given IPC surface to support a true before-write confirmation. M7's Templates tab
    approximates it: apply writes immediately, then offers an "Undo" (`ini_write_raw` back to
    the pre-apply raw, using the post-apply `mtimeMs` the reload picked up) rather than
    gating the write on a prior confirmation. Revisit if a dedicated preview command
    (`ini_template_preview`?) is added.
34. The Raw tab (`FR-INI-1`) uses a plain HTML `<textarea>`, which normalizes line endings
    to `\n` on read regardless of the file's actual `\r\n`/`\n` mix — a Raw-tab edit to a
    CRLF file could silently flip it to LF, unlike Form-tab edits (which go through
    `ini_apply`'s surgical, EOL-preserving patcher and are unaffected). Not verified against
    a real CRLF `platformio.ini` in this environment. Revisit if this turns out to matter in
    practice — the fix would be capturing/reinserting each line's original terminator around
    the textarea rather than trusting its `.value`.
35. `NFR-A3` ("all user-facing strings in one resource module from day one") is only
    partially satisfied. M8 adds `src/lib/strings.ts` and routes the *new* shell chrome
    through it (sidebar labels, canvas tabs, command palette, onboarding, the global
    settings screen's section names) — the feature-internal copy M2–M7 already shipped
    (chat block text, panel-specific labels across `ini`/`devices`/`pipeline`/`changes`)
    stays inline. A full retrofit of every pre-existing string was out of scope for this
    session; revisit before a real i18n pass is needed.
36. `FR-UI-9` ("multiple windows, one workspace each... `PortBroker` is global across
    windows") is implemented (`WebviewWindow` from `Workspace`'s "open in new window", a
    `workspace-*` capability pattern, and `?workspace=<id>` read by `App.tsx` to skip the
    Launcher) but not exercised against a second real OS window in this environment — this
    session's tooling can drive a Chrome tab, not a second native Tauri window, so "two
    windows open on two projects cannot both claim the same port" (the milestone's own
    acceptance wording) is architecturally true (one global `PortBroker`, already verified
    in M6) but not observed end-to-end with two actual windows. Worth a manual pass before
    relying on it.
37. The WCAG 2.1 AA contrast pass (`NFR-A2`) was a manual spot-check, not an automated
    audit (e.g. `axe-core`) — it found and fixed one systemic issue (bare `text-neutral-400`
    on a light background falls to ~2.5:1, below the 4.5:1 AA threshold for normal text;
    every occurrence across both M8's new shell code and earlier milestones' panels now
    pairs it with `text-neutral-500` in light mode) but wasn't exhaustive over every color
    combination in the app (badges, focus states, disabled controls). A real accessibility
    audit tool run is recommended before this milestone's acceptance claim is fully trusted.
38. `FR-CHAT-9` attachments has no file size or type limit — a user can attach an
    arbitrarily large file, which `core::claude::attachments::add` will happily copy into
    `.vibe/attachments/` and reference in the prompt text Claude's `Read` tool then fetches.
    Neither `SPEC.md` nor `CLI-CONTRACT.md` specifies a limit; none was invented. Revisit if
    a large attachment turns out to cause real problems (slow copies, a bloated `.vibe/`,
    or a prompt that blows past context).
