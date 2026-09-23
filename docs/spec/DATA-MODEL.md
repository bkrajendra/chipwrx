# DATA-MODEL.md — persistence and on-disk layout

Everything the app persists, where it lives, and what happens when it is corrupt.

Principles:
- **The workspace is the source of truth for the project.** The app's own state is a
  cache and an index, never the only copy of something the user cares about.
- **All writes are atomic** — temp file in the same directory, `fsync`, rename
  (NFR-R3).
- **Every file carries a `schemaVersion`.** Unknown future versions are read-only;
  older versions are migrated forward on load and re-written.
- A corrupt file is renamed to `<name>.corrupt-<ts>` and replaced with defaults. The app
  tells the user rather than silently losing state.

---

## 1. Application directories

Resolved via Tauri's path API, never hardcoded.

| Purpose | macOS | Windows | Linux |
|---|---|---|---|
| Config | `~/Library/Application Support/com.vibehardware.app/` | `%APPDATA%\vibe-hardware\` | `~/.config/vibe-hardware/` |
| Cache | `~/Library/Caches/com.vibehardware.app/` | `%LOCALAPPDATA%\vibe-hardware\Cache\` | `~/.cache/vibe-hardware/` |
| Logs | `~/Library/Logs/com.vibehardware.app/` | `%LOCALAPPDATA%\vibe-hardware\Logs\` | `~/.local/state/vibe-hardware/logs/` |

```
<config>/
  settings.json            # GlobalSettings
  projects.json            # ProjectRegistry
  templates/               # saved platformio.ini templates
    <slug>.json
<cache>/
  boards.json              # BoardCatalogue
  pio-schema-<version>.json# extracted ProjectOptions
  pio-targets/<ws-id>-<env>.json
  registry-search/         # keyed search responses, short TTL
<logs>/
  vibe-hardware.log        # rotating, 5 × 10 MB
```

---

## 2. Workspace-local state

Everything the app writes into the user's project lives under `.vibe/`, plus the
`CLAUDE.md` it generates (which belongs at the root, where Claude Code reads it).

```
<workspace>/
  platformio.ini           # user's, app edits in place, format-preserving
  CLAUDE.md                # generated context (FR-PROJ-8), user-editable
  src/ include/ lib/ test/ data/
  .pio/                    # PlatformIO build dir — never touched by the app
  .vibe/
    project.json           # ProjectSettings
    sessions/
      <session-id>.jsonl   # one TurnRecord per line, append-only
      index.json           # SessionIndex
    attachments/
      <uuid>-<original-name>
    builds.json            # BuildHistory (size trend, last good snapshot)
    shadow/                # bare-ish git dir used as snapshot store (if the
                           # workspace is not itself a git repo)
    .gitignore             # "*" — so .vibe never lands in the user's own repo
```

**`.vibe/.gitignore` containing `*` is mandatory.** Without it, the first `git add -A` in
the user's own repo commits their entire conversation history.

If the workspace *is* a git repo, the app also appends `.vibe/` to the repo's
`.git/info/exclude` (not `.gitignore`, which is the user's file).

---

## 3. `settings.json` — GlobalSettings

```jsonc
{
  "schemaVersion": 1,

  "toolchain": {
    "claudePath": "/Users/raj/.local/bin/claude",   // null = auto-resolve
    "pioPath": "/Users/raj/.platformio/penv/bin/pio",
    "pythonPath": null,
    "autoProbeOnFocus": true
  },

  "claude": {
    "permissionPolicy": "guarded",        // guarded | assisted | unrestricted
    "model": "sonnet",                     // alias or full model id
    "maxTurns": null,                      // null = CLI default
    "showThinking": false,
    "showCostEstimate": true
  },

  "pipeline": {
    "policy": "safe",                      // safe | fastPath | watch
    "autoReattachMonitor": true,
    "parallelJobs": null,                  // null = pio default (CPU count)
    "stopGraceMs": 3000                    // SIGINT → SIGTERM escalation window
  },

  "monitor": {
    "maxLines": 20000,
    "maxBytes": 8388608,
    "timestamps": false,
    "autoscroll": true
  },

  "logs": { "maxLines": 50000 },

  "editor": {
    "command": null,                       // null = auto-detect
    "gotoLineArgTemplate": "--goto {file}:{line}"
  },

  "appearance": { "theme": "system", "fontScale": 1.0, "monoFont": null },

  "network": {
    "registryTimeoutMs": 20000,
    "boardCatalogueTtlHours": 168
  },

  "advanced": {
    "keepProcessLogs": true,
    "allowUnrestrictedPolicy": false       // reset to false on every app update
  },

  "updates": { "checkForUpdates": true },  // NFR-D2: opt-out, doesn't gate the manual button

  "onboardingCompleted": false,
  "privacyNoticeAcknowledged": false        // NFR-S2: one-time disclosure, shown before onboarding
}
```

**Migration rule:** `claude.permissionPolicy` is forced back to `"guarded"` and
`advanced.allowUnrestrictedPolicy` to `false` whenever the app version changes
(FR-CHAT-4, NFR-S3).

---

## 4. `.vibe/project.json` — ProjectSettings

Per-workspace overrides. Any field absent falls back to the global setting.

```jsonc
{
  "schemaVersion": 1,
  "id": "0f9e2c6a-...",                  // WorkspaceId, stable across moves
  "name": "greenhouse-sensor",
  "activeEnv": "esp32dev",
  "device": {
    "preferredPort": "/dev/cu.usbserial-0001",
    "stickyHwid": { "vid": "10C4", "pid": "EA60", "serial": "0001" }
  },
  "claude": { "sessionId": "b1c0...", "permissionPolicy": null, "model": null },
  "pipeline": { "policy": null },
  "trusted": true,
  "trustScanAt": "2026-09-20T10:04:11Z",
  "createdBy": "vibe-hardware",           // vs "opened"
  "createdAt": "2026-09-18T07:22:00Z"
}
```

`id` is generated on first open and is what the registry keys on, so moving a folder does
not create a duplicate entry — the registry is reconciled by `id` first, path second.

---

## 5. `projects.json` — ProjectRegistry

```jsonc
{
  "schemaVersion": 1,
  "projects": [
    {
      "id": "0f9e2c6a-...",
      "name": "greenhouse-sensor",
      "path": "/Users/raj/Projects/greenhouse-sensor",
      "boardId": "esp32dev",
      "activeEnv": "esp32dev",
      "lastOpened": "2026-09-20T09:12:44Z",
      "lastBuildOk": true,
      "trusted": true
    }
  ]
}
```

A path that no longer exists is kept with `exists: false` at read time and shown greyed
with a "Locate…" action. Entries are never silently removed (FR-PROJ-4).

---

## 6. Session records

### `.vibe/sessions/<session-id>.jsonl`

Append-only, one JSON object per line. Append-only matters: a crash mid-turn loses at
most the current line, never the history.

```jsonc
// TurnRecord
{
  "schemaVersion": 1,
  "turnId": "3c1f...",
  "sessionId": "b1c0...",
  "startedAt": "2026-09-20T09:14:02.118Z",
  "endedAt": "2026-09-20T09:15:40.902Z",
  "prompt": "Read the DHT22 on GPIO 4 and publish temperature over MQTT every 30s",
  "attachments": ["attachments/6a1f-dht22-datasheet.pdf"],
  "model": "sonnet",
  "policy": "guarded",
  "argv": ["claude","-p","…"],            // redacted; for the diagnostics bundle
  "snapshotBefore": "a7f3e91",
  "events": [ /* the parsed ChatEvent stream, minus TextDelta noise */ ],
  "assistantText": "…",                    // reconstructed full prose
  "toolCalls": [
    { "toolUseId": "toolu_01…", "name": "Edit",
      "input": { "file_path": "src/main.cpp" }, "isError": false }
  ],
  "result": {
    "subtype": "success", "isError": false, "numTurns": 6,
    "durationMs": 98784, "durationApiMs": 71230,
    "totalCostUsd": 0.1842, "permissionDenials": []
  },
  "changes": [
    { "path": "src/main.cpp", "status": "modified", "additions": 64, "deletions": 9,
      "outsideExpectedDirs": false },
    { "path": "platformio.ini", "status": "modified", "additions": 2, "deletions": 0,
      "outsideExpectedDirs": false }
  ]
}
```

**Storage discipline.** Do not persist every `TextDelta`; store the reconstructed
`assistantText` and the structural events. A verbose turn can emit tens of thousands of
deltas, and the transcript file must stay openable.

### `.vibe/sessions/index.json`

```jsonc
{
  "schemaVersion": 1,
  "current": "b1c0...",
  "sessions": [
    { "id": "b1c0...", "startedAt": "…", "lastTurnAt": "…",
      "turnCount": 12, "totalCostUsd": 1.94, "title": "MQTT sensor loop" }
  ]
}
```

`title` is derived from the first prompt (first ~60 chars) and is user-editable.

---

## 7. `.vibe/builds.json` — BuildHistory

```jsonc
{
  "schemaVersion": 1,
  "lastGoodSnapshot": "a7f3e91",
  "builds": [
    {
      "id": "b-000124",
      "at": "2026-09-20T09:16:20Z",
      "env": "esp32dev",
      "kind": "build",                     // build | upload | test | check
      "success": true,
      "durationMs": 21430,
      "snapshot": "a7f3e91",
      "size": { "ramUsed": 44112, "ramTotal": 327680,
                "flashUsed": 812345, "flashTotal": 4194304 },
      "defectCount": { "error": 0, "warning": 3 }
    }
  ]
}
```

Capped at the most recent 200 entries; the size series feeds the sparkline in the
telemetry card (FR-BUILD-6).

---

## 8. `platformio.ini` model

Two representations, never conflated.

### 8.1 Declared — the editable model

Produced by a **format-preserving** parser. Each entry keeps its source span so the writer
can do surgical replacement, leaving comments, blank lines, key order, and unknown
sections byte-identical.

```
IniDocument {
  raw: String,
  mtimeMs: u64,
  sections: [ IniSection { name, entries: [ IniEntry { name, values[], span } ], comments } ]
}
```

Multi-value options use PlatformIO's continuation syntax — subsequent values are indented
under the key:

```ini
build_flags =
	-DDEBUG=1
	-Wall
lib_deps =
	bblanchon/ArduinoJson@^7.0.0
	knolleary/PubSubClient@^2.8
```

The writer must emit exactly this shape for `multiple: true` options (tab-indented
continuation lines), because that is what `pio project init -O` and `pio pkg install`
produce and mixing styles creates noisy diffs.

### 8.2 Effective — the read-only model

From `pio project config --json-output`, which returns a nested **array** with inheritance
already resolved (see `CLI-CONTRACT.md` §4.2). Reshape to:

```ts
Map<sectionName, Map<optionName, string | number | string[]>>
```

An option present in `effective` but not in `declared` for that section is **inherited**;
the UI shows it greyed with an "inherited from `[env]`" badge and an "override here"
action (FR-INI-3).

### 8.3 Option schema

`<cache>/pio-schema-<core_version>.json` — the output of the extraction one-liner in
`CLI-CONTRACT.md` §6, verbatim:

```jsonc
{
  "platformio.name":   { "scope":"platformio","group":"generic","name":"name", "...": "..." },
  "env.build_flags":   { "scope":"env","group":"build","name":"build_flags",
                         "type":"string","multiple":true,"default":null,
                         "sysenvvar":"PLATFORMIO_BUILD_FLAGS","description":"…" },
  "env.lib_ldf_mode":  { "type":"choice","multiple":false,"default":"chain",
                         "choices":["off","chain","deep","chain+","deep+"], "...": "..." }
}
```

A bundled fallback copy (captured from Core 6.2.0) ships with the app so the Form tab
still renders when extraction fails.

---

## 9. `<cache>/boards.json` — BoardCatalogue

```jsonc
{
  "schemaVersion": 1,
  "fetchedAt": "2026-09-20T08:00:00Z",
  "pioVersion": "6.2.0",
  "source": "registry",                 // registry | installed
  "boards": [ /* BoardBrief[] — see CLI-CONTRACT §3.1 */ ]
}
```

TTL from `network.boardCatalogueTtlHours` (default 7 days). If the fetch fails, the stale
cache is used and the UI shows "catalogue from <date>" with a Refresh action. If there is
no cache at all, fall back to `pio boards --installed --json-output`.

Build a search index on load: lowercase concatenation of `id`, `name`, `mcu`, `vendor`,
`platform`, and `frameworks`, so filtering stays under 50 ms for ~1 500 entries (NFR-P4).

---

## 10. Templates — `<config>/templates/<slug>.json`

```jsonc
{
  "schemaVersion": 1,
  "name": "ESP32 + MQTT starter",
  "createdAt": "2026-09-14T11:02:00Z",
  "ini": "[env:esp32dev]\nplatform = espressif32\n…",
  "boardId": "esp32dev",
  "claudeMd": "…optional context block…"
}
```

Applying a template merges `[env:*]` sections into the current file rather than replacing
it, and always shows the resulting diff for confirmation before writing.

---

## 11. Generated `CLAUDE.md`

Written at the workspace root on creation and on board change (FR-PROJ-8). Between
markers so user edits outside them survive regeneration:

```markdown
<!-- vibe-hardware:begin (generated — edits inside this block are overwritten) -->
# Hardware context

Target board: **esp32dev** — Espressif ESP32 Dev Module (Espressif)
MCU: ESP32 @ 240 MHz · Flash 4 MB · RAM 320 KB
Frameworks available: arduino, espidf · Active: arduino
Connectivity: wifi, bluetooth, ethernet, can

## Build system
PlatformIO. Do not invent a Makefile or CMakeLists — `platformio.ini` is the build
configuration.

Current `platformio.ini`:

```ini
[env:esp32dev]
platform = espressif32
board = esp32dev
framework = arduino
monitor_speed = 115200
```

## House rules
- Application code goes in `src/`. Shared headers in `include/`. Private libraries in
  `lib/<Name>/`. Tests in `test/`.
- Add every external library to `lib_deps` in `platformio.ini`. Never vendor sources
  into `src/`.
- This target is memory-constrained: avoid dynamic allocation inside `loop()`, prefer
  fixed-size buffers, and keep ISRs short and `IRAM_ATTR` where the platform requires it.
- Do not write to `.pio/` or `.vibe/`.
- Do not change `board`, `platform`, `upload_protocol`, or flash layout options unless
  explicitly asked — those can make the device unflashable.
- After changing code, do not run the build yourself; the user triggers Build and Upload
  from the app.
<!-- vibe-hardware:end -->
```

---

## 12. Migration and integrity

| Situation | Behaviour |
|---|---|
| `schemaVersion` lower than current | Migrate in memory, re-write atomically, log the migration |
| `schemaVersion` higher than current | Load read-only; banner: "This project was used with a newer version of Vibe Hardware" |
| JSON parse failure | Rename to `<name>.corrupt-<unix-ts>`, recreate defaults, toast the user with the path to the salvaged file |
| `sessions/*.jsonl` partial final line | Truncate to the last complete line on load; the interrupted turn is recoverable via `--resume` |
| Shadow repo missing or broken | Recreate it; snapshots before that point are gone, so the Changes tab shows "history unavailable before <date>" rather than failing |
| Workspace moved | Reconcile by `id` from `.vibe/project.json`; update the registry path silently |

---

## 13. What is deliberately **not** persisted

- Anthropic credentials. Auth lives entirely in the Claude CLI's own store.
- `ANTHROPIC_API_KEY`, if supplied — OS keychain only, never a config file (NFR-S1).
- Build artefacts. `.pio/` belongs to PlatformIO.
- Anything sent off the machine. There is no telemetry endpoint in v1 (NFR-S2).
