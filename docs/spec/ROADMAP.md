# ROADMAP.md — milestones

Eleven milestones. Each one is independently demoable and ends in an acceptance test you
can run. Give Claude Code **one milestone per session**.

Legend: **FR-*** references `SPEC.md` §5.

---

## M0 — Skeleton and harness

**Goal:** a window, a build, and the ability to test everything that follows without
hardware, network, or cost.

- Tauri v2 + React 19 + TS + Vite scaffold; Tailwind + Radix; `tauri-plugin-log`.
- `ProcessSupervisor` (`ARCHITECTURE.md` §3): argv-only spawn, process groups / job
  objects, streaming stdout+stderr split on `\n` **and** `\r`, SIGINT/SIGTERM escalation,
  process table, kill-all on exit.
- `ts-rs` binding generation wired into the build; CI fails on drift.
- **`FakeCli` harness**: a small Rust binary that replays a fixture file to stdout with
  recorded timing, selectable via the resolved-path setting. Fixtures for: a successful
  `pio run`, a failing `pio run`, `pio device list --json-output`, and a full `claude -p`
  NDJSON transcript.
- `AppError` enum and the frontend error-renderer registry.
- CI: `cargo test`, `cargo clippy -D warnings`, `vitest`, `tsc --noEmit`.

**Acceptance:** `npm run tauri dev` opens a window. A test spawns `FakeCli`, streams 10 000
lines into a channel, cancels mid-stream, and asserts every child process is gone.

---

## M1 — Doctor and toolchain

**FR-SETUP-1..7, 9**

- All nine probes, concurrent, timeout-bounded.
- Binary resolution including the login-shell fallback (`TOOLCHAIN-SETUP.md` §3).
- Doctor screen: probe list, statuses, remediations.
- Install flows for Claude Code and PlatformIO with streamed output and a visible command
  preview.
- Auth detection via probe turn; the three remediation paths.
- Version floors and the feature-gate table.
- Diagnostics bundle export.

**Acceptance:** on a VM with neither CLI installed, the app installs both from the UI,
detects unauthenticated Claude, guides through sign-in, and ends with every probe green.
With the network blocked, Doctor reports `networkRegistry: Degraded` within 20 s and the
app stays responsive.

---

## M2 — Projects

**FR-PROJ-1..7, NFR-P4**

- `pio boards --json-output` fetch, cache, TTL, offline fallback to `--installed`.
- Board picker with the prebuilt search index; filters for platform, framework, MCU,
  vendor, connectivity; Flash/RAM/frequency columns.
- Create wizard → `pio project init` with streamed progress, cancellable, with the
  "first platform download is large" explanation.
- Open existing folder; `NotAPioProject` path with an offer to initialize.
- Project registry with `id`-first reconciliation; missing paths flagged, never dropped.
- Environment switcher from `platformio.ini`.
- Open in editor / Reveal, with editor auto-detection.
- Workspace trust scan (`NFR-S5`) before anything runs in an adopted folder.

**Acceptance:** create an `esp32dev` project into a chosen folder; the toolchain download
streams with a working Cancel; reopening the app lists the project; "Open in editor"
launches the user's editor. With the network down, the board picker still lists installed
platforms' boards and says so.

---

## M3 — Conversational loop

**FR-CHAT-1..3, 5, 6, 10, 11**

- Turn runner: argv construction per `CLI-CONTRACT.md` §1.4, cwd = workspace,
  `--session-id` / `--resume`, never `--bare`.
- NDJSON parser: incremental, unknown-tolerant, huge-line safe, delta coalescing on a
  ~16 ms tick.
- `ChatEvent` channel and the chat UI: streaming prose, tool-call cards, `api_retry`
  chips, result footer with duration / turns / cost estimate.
- Stop → SIGINT, escalate to SIGTERM; exit-143 surfaced as "turn interrupted, will resume".
- Session persistence to `.vibe/sessions/*.jsonl`; history restored on reopen; New session.
- Prompt deck: multi-line, `Cmd/Ctrl+Enter`, per-project history, disabled-while-running.

**Acceptance:** against the `FakeCli` transcript, a turn streams token-by-token at frame
rate, tool cards appear in order, Stop halts mid-stream and the UI says the turn was
interrupted, and restarting the app restores the conversation.

---

## M4 — Permissions and the safety net

**FR-CHAT-4, FR-SAFE-1..6, NFR-S5**

This milestone is where the product becomes safe to use on real work. Do not defer it.

- Three permission policies with correct flag mapping; `--permission-prompts none` gated
  on version; Unrestricted behind a typed confirmation and reset on update.
- `permission_denied` events rendered as a first-class state, not an error.
- Snapshot subsystem: shadow git for non-repos, `refs/vibe/snapshots/*` for repos,
  `.vibe/.gitignore` containing `*`, `.git/info/exclude` entry.
- Pre-turn snapshot; post-turn diff; Changes tab with per-file read-only diff viewer.
- Per-file and whole-turn revert, itself snapshotted.
- Out-of-expected-dirs warning banner; ini-aware diff for `platformio.ini`.

**Acceptance:** a turn that edits three files produces a Changes tab with correct line
counts; reverting one file restores it byte-for-byte; reverting the turn restores all
three; the user's own `git status` is unchanged throughout. A turn run in Guarded policy
against a fixture that attempts an arbitrary `Bash` call surfaces a `permissionDenied`
card rather than failing silently.

---

## M5 — Build and upload

**FR-BUILD-1..8, FR-UI-4**

- `pipeline_build` / `pipeline_upload` with ANSI-preserving streaming into an xterm.js pane.
- Pipeline state machine (`ARCHITECTURE.md` §4.1) and the pipeline strip with elapsed time
  and Stop.
- Safe / Fast-path / Watch policies, including Safe-mode invalidation on file change.
- Diagnostic parser (compiler + linker forms) → Problems tab, with source peek and
  "open in editor at line".
- Size parser → telemetry card with bars and delta vs previous good build;
  `.vibe/builds.json`.
- **"Ask Claude to fix these N errors"** composing a prompt from structured defects plus
  the output tail.
- Extra-targets menu with cached `--list-targets`, universal subset before that.
- Cancellation kills the whole process tree.

**Acceptance:** with real hardware, Build → Upload flashes an ESP32 and the size card
shows usage. With a deliberate syntax error, Problems lists it with file:line, and the fix
button produces a turn whose next build succeeds. Stop during a build leaves no compiler
processes behind (verified with `ps`).

---

## M6 — Devices, telemetry, monitor

**FR-DEV-1..9, NFR-P3**

- `pio device list --serial --json-output` polling at the two cadences; `hwid` parsing;
  VID:PID bridge table; hot-plug toasts.
- Device stickiness by VID:PID+serial.
- `PortBroker` with RAII leases, monitor preemption, post-flash re-enumeration retry with
  backoff.
- In-app monitor over the `serialport` crate, driven by the env's `monitor_*` options:
  xterm.js rendering, timestamps, autoscroll, regex filter, ring buffer caps, clear, save
  log, send box honouring EOL.
- "Send selection to Claude".
- Telemetry adapters: ESP (`pio pkg exec -p tool-esptoolpy -- esptool.py|esptool flash_id`,
  probing both entrypoints, stripping the `Using …package` prefix) and generic.
- "Open in external terminal" via `pio device monitor`.

**Acceptance:** with the monitor running, pressing Upload stops the monitor, flashes,
and reattaches automatically, narrated in the UI. Unplugging mid-monitor produces a
specific message and re-plugging re-binds to the same board on a different port path. On a
non-ESP board the telemetry card shows generic identity with no error.

---

## M7 — `platformio.ini` and packages

**FR-INI-1..11**

- Format-preserving INI parser/writer with a property test: parse→write is byte-identical
  over a fixture corpus of real `platformio.ini` files.
- Option schema extraction (`CLI-CONTRACT.md` §6), version-keyed cache, bundled fallback.
- Form tab generated from the schema: widget mapping by `type` × `multiple`, PlatformIO's
  own descriptions as help text, defaults as placeholders.
- Effective column from `pio project config --json-output`, inheritance badges, "override
  here".
- Raw tab with two-way sync and the mtime-changed-on-disk guard.
- Lint via `pio project config --lint` **without** `--json-output` (the JSON form emits a
  Python repr — do not `JSON.parse` it).
- Library browser against `api.registry.platformio.org/v3/search`, qualifier chips,
  install/uninstall via `pio pkg install|uninstall -l`, version pinning selector.
- Installed/outdated lists from `pio pkg list` / `pio pkg outdated` (text parsing).
- Global PlatformIO settings screen over `pio settings get/set` with per-setting reset.
- Templates: save, list, apply-with-diff-confirmation.

**Acceptance:** adding `-DFOO=1` to `build_flags` through the Form tab leaves every
comment, blank line, and unrelated section byte-identical in the file. Setting
`monitor_speed` in `[env]` shows as inherited in `[env:esp32dev]` with an override action.
Installing ArduinoJson from the browser adds it to `lib_deps` exactly once and the next
build compiles it.

---

## M8 — Shell polish

**FR-UI-1..9, NFR-A1..A3, NFR-P1..P3**

- Final two-pane layout, resizable and collapsible sidebar, all six canvas tabs.
- Command palette (`Cmd/Ctrl+K`) and the full shortcut set.
- Light/dark themes following the OS; WCAG 2.1 AA contrast audit in both.
- Keyboard-only pass over every flow; focus rings; ARIA labels on telemetry values.
- Log and monitor virtualisation with the configured caps.
- Multi-window with a single shared `PortBroker`.
- All strings through the i18n resource module.
- Onboarding (`TOOLCHAIN-SETUP.md` §9).
- Attachments (FR-CHAT-9) and slash passthrough (FR-CHAT-8).

**Acceptance:** an accessibility audit reports no AA contrast failures and no
keyboard traps. A 200 KB assistant response renders without dropping below 30 fps. Two
windows open on two projects cannot both claim the same port.

---

## M9 — Hardening

**NFR-R1..R4, NFR-S1..S5, FR-BUILD-9/10**

- `pio check --json-output` and `pio test --json-output` wired into Problems / a test view.
- Secret redaction filter in the log sink; keychain storage for `ANTHROPIC_API_KEY`.
- Tauri capabilities audited down to the minimum; no `shell`, no global `fs`.
- Path canonicalisation assertions on every filesystem operation.
- Atomic writes and the corruption-recovery paths in `DATA-MODEL.md` §12.
- Orphan-process reaping on panic and on `RunEvent::Exit`.
- Offline behaviour verified end to end.
- Schema migration tests from v0 fixtures.

**Acceptance:** killing the app with `SIGKILL` mid-build leaves no orphaned processes on
next launch. A truncated `sessions/*.jsonl` loads cleanly with the partial turn dropped. A
log containing a fake API key exports with it redacted.

---

## M10 — Packaging and release

**NFR-D1..D3**

- macOS universal `.dmg`, signed and notarized; Windows MSI/NSIS Authenticode-signed;
  Linux `.AppImage` + `.deb`.
- Tauri updater with a signed manifest; opt-out setting.
- Release workflow producing reproducible builds; version + git SHA in About and in the
  diagnostics bundle.
- First-run trust/privacy notice stating that prompts and the code Claude reads are sent
  to Anthropic by the Claude CLI.
- README, a 90-second demo recording, and the hardware smoke-test checklist below.

**Acceptance:** a fresh macOS VM installs the DMG, passes Gatekeeper, completes
onboarding, and flashes a board without a terminal. An in-app update from the previous
version preserves projects, sessions, and settings — and resets the permission policy to
Guarded.

---

## Hardware smoke-test matrix (every release)

| Board | Platform | Checks |
|---|---|---|
| ESP32-DevKitC (CP2102) | espressif32 | create, build, upload, monitor, `flash_id` telemetry, monitor preemption |
| ESP8266 NodeMCU (CH340) | espressif8266 | as above; CH340 driver path on Windows |
| Arduino Uno (ATmega328P) | atmelavr | create, build, upload, monitor; generic telemetry; no esptool errors |
| Raspberry Pi Pico (RP2040) | raspberrypi | UF2 upload path, port disappearing after flash |
| STM32 BluePill (ST-Link) | ststm32 | non-serial upload protocol; Upload works with no serial port selected |

Run each on macOS, Windows 11, and Ubuntu LTS.

---

## Ordering constraints

```
M0 ─┬─► M1 ──► M2 ──► M3 ──► M4 ──► M5 ──► M6 ──► M8 ──► M9 ──► M10
    └─────────► M7 ───────────────────────────────┘
```

M7 depends only on M2 (a project must exist) and can run in parallel with M3–M6 if a
second implementer is available. **M4 must not be deferred past M5** — the moment builds
and uploads work, unsupervised agent edits start touching hardware.
