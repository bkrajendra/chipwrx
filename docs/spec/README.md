# Vibe Hardware — Specification Pack

Working name: **Vibe Hardware** (codename `vhw`). An editorless desktop shell for
conversational embedded development: React + Tauri v2 driving the **PlatformIO Core
CLI** and the **Claude Code CLI** over a Rust command bridge.

This pack is the v3 specification. It takes the v2 high-level spec as input, performs a
production-readiness gap analysis, and expands it into something a coding agent can
implement milestone by milestone.

---

## Read this in order

| # | Document | What it settles |
|---|---|---|
| 1 | [`SPEC.md`](./SPEC.md) | Product scope, gap analysis of v2, numbered functional requirements (FR-*), screen-by-screen UX, non-functional requirements |
| 2 | [`ARCHITECTURE.md`](./ARCHITECTURE.md) | Layer diagram, crate/module layout, process supervision, the five state machines, serial-port arbitration, safety net |
| 3 | [`CLI-CONTRACT.md`](./CLI-CONTRACT.md) | **Every** external CLI invocation, verbatim and verified, with JSON shapes, exit codes, and known gotchas |
| 4 | [`IPC-CONTRACT.md`](./IPC-CONTRACT.md) | Tauri command signatures, `Channel<T>` event enums, matching TypeScript types |
| 5 | [`DATA-MODEL.md`](./DATA-MODEL.md) | On-disk layout, settings schemas, project registry, session/transcript records, `platformio.ini` model |
| 6 | [`TOOLCHAIN-SETUP.md`](./TOOLCHAIN-SETUP.md) | Doctor + auto-install flows for Claude Code and PlatformIO Core, auth, udev/driver handling |
| 7 | [`ROADMAP.md`](./ROADMAP.md) | M0–M10 milestones, each with a demoable acceptance test |
| 8 | [`CLAUDE.md`](./CLAUDE.md) | Repo conventions — copy this to the repo root before you start |

---

## Decisions already locked

These were open questions in v2. They are now closed; do not re-litigate them during
implementation.

| Question | Decision |
|---|---|
| Claude bridge | `claude -p --output-format stream-json --verbose --include-partial-messages`, one process per turn, continuity via `--session-id` / `--resume`. No Agent SDK sidecar, no raw API key path in v1. |
| Board scope | Every board PlatformIO knows (`pio boards --json-output`). ESP32/ESP8266 get a richer telemetry adapter; all other families degrade to generic serial identity. |
| PlatformIO install | Detect and adopt an existing install first. Otherwise run the official `get-platformio.py` into an app-managed `~/.platformio/penv`. No bundled Python. |
| Build/upload trigger | Manual by default. `Build → Upload` gating is a setting (Safe / Fast-path / Watch). |
| Concurrency | One active board per window. Multiple projects = multiple windows. |
| Code editing | None in-app. "Open in editor" hands off to the user's editor. A **read-only** diff viewer is not an editor and is required. |

---

## How to drive Claude Code with this pack

```bash
git init vibe-hardware && cd vibe-hardware
cp /path/to/vibe-hardware-spec/CLAUDE.md .
mkdir -p docs/spec && cp /path/to/vibe-hardware-spec/*.md docs/spec/

claude
```

Then, one milestone per session:

```
Read docs/spec/CLAUDE.md, docs/spec/ARCHITECTURE.md and docs/spec/ROADMAP.md.
Implement milestone M1 only. Stop at its acceptance test and show me how to run it.
```

Keep sessions milestone-scoped. The pack is deliberately split so that a session
implementing the `platformio.ini` editor loads `DATA-MODEL.md` + `CLI-CONTRACT.md` and
not the whole corpus.

---

## Verification status of external facts

Everything in `CLI-CONTRACT.md` marked **[V]** was verified on 2026-09-20 against a live
**PlatformIO Core 6.2.0** install (help output and package source read directly) or
against current Claude Code documentation. Items marked **[U]** are unverified and must
be probed at runtime by the app's doctor rather than assumed.
