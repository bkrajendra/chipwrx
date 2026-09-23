<div align="center">

# Vibe Hardware

**Conversational embedded development.** Talk to Claude Code, build and flash with
PlatformIO — no code editor in the product.

[![CI](https://github.com/bkrajendra/chipwrx/actions/workflows/ci.yml/badge.svg)](https://github.com/bkrajendra/chipwrx/actions/workflows/ci.yml)
[![Release](https://github.com/bkrajendra/chipwrx/actions/workflows/release.yml/badge.svg)](https://github.com/bkrajendra/chipwrx/actions/workflows/release.yml)
[![Latest release](https://img.shields.io/github/v/release/bkrajendra/chipwrx?include_prereleases&sort=semver)](https://github.com/bkrajendra/chipwrx/releases/latest)
[![Tauri v2](https://img.shields.io/badge/Tauri-v2-24C8DB?logo=tauri&logoColor=white)](https://v2.tauri.app/)
[![React 19](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=white)](https://react.dev/)
[![Rust](https://img.shields.io/badge/Rust-core-DEA584?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-informational)](#download)

[**Download**](https://bkrajendra.github.io/chipwrx/) ·
[Documentation](docs/spec/) ·
[Report an issue](https://github.com/bkrajendra/chipwrx/issues)

</div>

---

<p align="center">
  <img src="docs/screenshot.png" alt="Vibe Hardware — chat, build, and monitor a board in one window" width="820" />
  <br />
  <sub><em>Screenshot coming soon — drop one at <code>docs/screenshot.png</code> and this shows up automatically.</em></sub>
</p>

---

## What it does

- **Chat with Claude Code about your firmware.** It edits files under your explicit
  permission policy, and every turn is snapshotted so you can revert it, per file or whole.
- **Build and upload through PlatformIO** with streamed, ANSI-preserving output; compiler
  and linker errors land in a structured Problems tab with an "ask Claude to fix these"
  button.
- **In-app serial monitor and telemetry**, per board (ESP32/ESP8266 get a richer adapter;
  everything else PlatformIO knows about gets generic identity).
- **A form-driven `platformio.ini` editor** generated from PlatformIO's own option schema,
  plus a byte-identical raw editor and a library browser.

## At a glance

| | |
|---|---|
| **Shell** | Tauri v2 — Rust core, React 19 + TypeScript renderer |
| **Drives** | PlatformIO Core CLI, Claude Code CLI |
| **Platforms** | macOS (universal `.dmg`), Windows (`.msi`/NSIS), Linux (`.AppImage`/`.deb`) |
| **Updates** | In-app, signed manifest, opt-out |
| **Status** | Milestones M0–M10 complete — see [`ROADMAP.md`](docs/spec/ROADMAP.md) |

## Download

**[Get the latest release →](https://bkrajendra.github.io/chipwrx/)**

Builds are published from [Releases](https://github.com/bkrajendra/chipwrx/releases) on
every tagged version. The app checks for updates on startup (opt-out in Settings → About)
and applies them in place.

See [`docs/spec/`](docs/spec/) for the full specification this was built from —
`SPEC.md` for functional requirements, `ARCHITECTURE.md` for the shape of the code, and
`ROADMAP.md` for how it was built, milestone by milestone.

## Developing

```bash
npm install
npm run tauri dev
```

Requires the [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your
platform. Nothing here needs real hardware, network access, or a paid API call — tests run
against `FakeCli`, a fixture-replaying stand-in for both `pio` and `claude`.

```bash
npm run test          # vitest
npm run typecheck     # tsc --noEmit
npm run test:rust     # cargo test
npm run lint:rust     # cargo clippy -D warnings
npm run gen:bindings  # regenerate src/lib/bindings.ts from the Rust IPC types
```

## Releasing

Push a tag matching `v*.*.*` (e.g. `v0.2.0`). [`release.yml`](.github/workflows/release.yml)
builds all three platforms, stamps the version from the tag, signs what it can (Apple
notarization, Windows Authenticode, and the updater manifest all need real credentials in
repo secrets — see that workflow's comments for which ones), and opens a **draft** GitHub
Release with every installer attached for review before publishing. Publishing it triggers
[`pages.yml`](.github/workflows/pages.yml), which re-renders the download page above.

Before a release, run the hardware smoke-test matrix in
[`docs/spec/ROADMAP.md`](docs/spec/ROADMAP.md#hardware-smoke-test-matrix-every-release) on
real boards — CI can't do this part for you.
