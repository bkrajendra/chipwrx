# CLAUDE.md — Vibe Hardware

Copy this to the repository root. It is the standing context for every Claude Code
session in this repo.

---

## What this is

A Tauri v2 desktop app that drives the **PlatformIO Core CLI** and the **Claude Code
CLI** to build embedded firmware conversationally. React + TypeScript renderer, Rust core.
No code editor in the product.

Full specification lives in `docs/spec/`. Read the documents you need — do not load all of
them:

| Working on | Read |
|---|---|
| Anything | `docs/spec/ARCHITECTURE.md` |
| A feature's behaviour | `docs/spec/SPEC.md` (find the FR-* ids) |
| Any external command | `docs/spec/CLI-CONTRACT.md` — **always**, before writing an invocation |
| A Tauri command or event | `docs/spec/IPC-CONTRACT.md` |
| Anything persisted | `docs/spec/DATA-MODEL.md` |
| Doctor / install / auth | `docs/spec/TOOLCHAIN-SETUP.md` |
| What to build next | `docs/spec/ROADMAP.md` |

---

## Hard rules

1. **Never invent a CLI flag.** Every `pio` and `claude` invocation must match
   `CLI-CONTRACT.md`. If something you need is not in there, stop and say so — do not
   guess a flag and do not assume a command supports `--json-output`. Several do not
   (`pio pkg search`, `pio pkg list`, `pio pkg outdated`, `pio run --list-targets`,
   `pio settings get`), and one is actively broken
   (`pio project config --lint --json-output` emits a Python repr, not JSON).

2. **No shell strings.** Every external command is an argv array with an absolute program
   path, spawned through `ProcessSupervisor`. There is no `Command::new` anywhere outside
   `core/proc`. No string interpolation of paths, ports, or environment names into a
   command.

3. **Rust owns the types.** IPC types are defined in Rust and exported with `ts-rs` into
   `src/lib/bindings.ts`. Never hand-write a TypeScript mirror of a Rust type. If the
   generated bindings drift, regenerate — do not edit the generated file.

4. **`invoke` only in `src/lib/ipc.ts`.** Feature code imports typed wrappers.

5. **No `unwrap()` / `expect()` in non-test Rust.** Return `AppError`. Panicking in a
   Tauri command takes down the window.

6. **Errors are typed, never stringified in Rust.** Return the `AppError` variant; the UI
   decides the wording. There is no generic "Something went wrong" dialog in this product.

7. **`--bare` is forbidden** when invoking the Claude CLI: it skips `CLAUDE.md` and never
   reads OAuth credentials, so subscription auth stops working.

8. **Never pass a Claude turn without an explicit `--permission-mode`.** `-p` starts in
   Manual mode and denies anything that would prompt, so the agent writes no files and the
   failure is silent. See `SPEC.md` G2.

9. **Kill process groups, not PIDs.** SCons spawns compilers as grandchildren.

10. **Never run `sudo`.** Show the command; let the user run it.

11. **Never write to the user's `.git/`, `.gitignore`, or `.pio/`.** App state goes in
    `.vibe/`, which contains a `.gitignore` of `*`.

12. **Secrets never reach a log sink unredacted.** The redaction filter runs before
    anything hits disk or the UI.

---

## Stack and conventions

| Concern | Choice |
|---|---|
| Shell | Tauri v2 (plugins: `dialog`, `fs` (scoped), `store`, `opener`, `updater`, `single-instance`, `log`, `os`, `process`, `window-state`). **Not** `shell`. |
| UI | React 19, TypeScript strict, Vite, Tailwind, Radix primitives |
| State | Zustand slices (one per state machine in `ARCHITECTURE.md` §4), TanStack Query for CLI-backed reads |
| Terminal panes | xterm.js — do not hand-roll ANSI |
| Diff viewer | read-only CodeMirror 6 + `diff`. A read-only diff viewer is not "a code editor"; an editable one would violate the product's core constraint |
| Async | tokio |
| Serial | `serialport` crate, in-process. **Not** by scraping `pio device monitor` |
| Git | `git2`, CLI fallback |
| Logging | `tracing` + `tauri-plugin-log` |

### Naming

- Rust: modules `snake_case`, types `PascalCase`. Tauri commands are `<area>_<verb>`
  (`project_create`, `pipeline_build`, `ini_apply`).
- Serde: `#[serde(rename_all = "camelCase")]` on every IPC type; enums are tagged
  `tag = "type", content = "data"`.
- TypeScript: components `PascalCase`, hooks `useX`, stores `<name>Store`.
- Files: one feature per directory under `src/features/`.

### Layout

```
src/                 React
src-tauri/src/
  commands/          thin Tauri command handlers — no logic
  core/              all logic; unit-testable without Tauri
    proc/ toolchain/ claude/ pio/ project/ ini/ device/ snapshot/ diag/
tests/fixtures/      golden CLI output and NDJSON transcripts
docs/spec/           the specification pack
```

Command handlers stay thin. If a handler is more than ~20 lines, the logic belongs in
`core/`.

---

## Testing

- **Everything goes through `FakeCli`.** It replays golden fixtures with realistic timing
  and is substituted for the real binary via the resolved-path setting. No test may
  require network, hardware, or a paid API call.
- Parsers get golden-file tests: `pio run` success/compile-error/link-error/size output,
  `pio device list --json-output`, `pio boards --json-output`, and a full `claude -p`
  NDJSON transcript including malformed, partial, and very large lines.
- The INI writer gets a property test: parse→write is byte-identical across the fixture
  corpus.
- `cargo test`, `cargo clippy -D warnings`, `vitest`, `tsc --noEmit` all pass before any
  commit.

---

## Working style in this repo

- **One milestone per session.** Read the milestone in `ROADMAP.md`, implement it, stop at
  its acceptance test, and show how to run it.
- Prefer small, reviewable commits with the FR-* or milestone id in the message.
- When a spec document and the code disagree, the spec wins — or say so and propose a spec
  change. Do not silently diverge.
- When you hit something the spec does not cover, add it to the "Open questions" section
  of `SPEC.md` rather than inventing a behaviour and moving on.
- Do not add dependencies casually. New crates and npm packages need a one-line
  justification in the commit message.
- Do not create README or documentation files unless asked.

---

## Landmines, in order of how much time they will cost you

1. **`claude -p` writes nothing without an explicit permission mode.** The run "succeeds"
   and returns prose. You will lose an afternoon to this.
2. **`pio project config --lint --json-output` is not JSON.** Single-quoted Python repr.
3. **`pio device list --json-output` changes shape** depending on how many device kinds
   were requested. Always pass exactly `--serial`.
4. **`pio project metadata` without `--json-output` installs dependencies** as a side
   effect.
5. **`pio project init -b <board>` needs the registry**; offline it hangs ~60 s then fails
   with a bare `HTTPClientError:`.
6. **`pio run --list-targets` has no JSON form** and needs the platform installed.
7. **Killing the `pio` PID leaves compilers running.** Process groups.
8. **The macOS `.app` `PATH` is not your terminal's `PATH`.** Resolve binaries explicitly.
9. **Uploading while the monitor holds the port fails.** Everything goes through
   `PortBroker`.
10. **Boards re-enumerate after a flash** — the port vanishes and returns after
    ~0.5–1.5 s. Retry with backoff before reporting a lost device.
11. **`esptool.py` vs `esptool`** — the entrypoint name is not stable across
    `tool-esptoolpy` versions. Probe both; strip the `Using …package` prefix line.
12. **`claude -p` runs a workspace's hooks and MCP servers with no prompt.** Trust-scan
    any folder the app did not create before the first turn.
