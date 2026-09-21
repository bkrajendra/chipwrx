# CLI-CONTRACT.md — external command surface

Every external command the app may run, with exact flags, output shapes, exit behaviour,
and gotchas.

**Verification legend**

- **[V]** Verified 2026-09-20 against a live **PlatformIO Core 6.2.0** install (`--help`
  output and package source read directly) or against current Claude Code documentation.
- **[U]** Unverified — must be probed at runtime, not assumed.

> Implementation rule: nothing in this file is invoked as a shell string. Every command
> is an argv array passed to `ProcessSupervisor` with an absolute program path.

---

## 1. Claude Code CLI

### 1.1 Detection **[V]**

```
claude --version        → "2.1.211 (Claude Code)"  (version, space, "(Claude Code)")
claude doctor           → read-only installation and settings diagnostics, no session
```

Parse the version with `^(\d+\.\d+\.\d+)` and compare semver. Do not parse `claude doctor`
output for logic — capture it verbatim for the diagnostics bundle only.

Native installs live at:

| OS | Launcher | Versions |
|---|---|---|
| macOS/Linux | `~/.local/bin/claude` | `~/.local/share/claude/versions/` |
| Windows | `%USERPROFILE%\.local\bin\claude.exe` | `%USERPROFILE%\.local\share\claude\` |

Other install methods put it elsewhere (Homebrew cask, WinGet, apt/dnf/apk, npm global),
so **always** resolve the path rather than assuming.

### 1.2 Install **[V]**

| OS | Command |
|---|---|
| macOS / Linux / WSL | `curl -fsSL https://claude.ai/install.sh \| bash` |
| Windows PowerShell | `irm https://claude.ai/install.ps1 \| iex` |
| Windows CMD | `curl -fsSL https://claude.ai/install.cmd -o install.cmd && install.cmd && del install.cmd` |
| Homebrew | `brew install --cask claude-code` |
| WinGet | `winget install Anthropic.ClaudeCode` |
| npm | `npm install -g @anthropic-ai/claude-code` (Node 22+) |

Append a channel or version to the native installer: `... | bash -s stable`,
`... | bash -s 2.1.89`.

Native installs auto-update in the background. Package-manager installs do not.

### 1.3 Authentication **[V]**

Claude Code requires a Pro, Max, Team, Enterprise, or Console account. **There is no
headless interactive login.** The app's options:

1. Open a terminal running `claude` and let the user complete the browser flow.
2. `claude setup-token` — generates a long-lived OAuth token for scripts.
3. `ANTHROPIC_API_KEY` in the environment — Claude Code prompts once to approve it.

Detection: run a trivial, near-free probe turn and inspect the result. A failure inside
the run (including missing auth) is **printed as the result on stdout** with a non-zero
exit — it is not an argument error on stderr. Treat `is_error: true` plus an auth-shaped
message, or a `system/api_retry` with `error: "authentication_failed"`, as unauthenticated.

### 1.4 The turn invocation **[V]**

```
<claude> -p "<prompt>"
  --output-format stream-json
  --verbose                        # required alongside stream-json
  --include-partial-messages       # token-level deltas; requires -p + stream-json
  --permission-mode <mode>
  --allowedTools "<rules>"
  --permission-prompts none        # v2.1.259+ only; omit on older
  --model <alias|id>
  [--session-id <uuid> | --resume <uuid>]
```

Working directory = the workspace. Do **not** pass `--add-dir` unless the user has
explicitly added a second directory; the absence of it is part of the sandbox (FR-SAFE-4).

**Never pass `--bare`.** Bare mode skips `CLAUDE.md`, skills, and plugins, and — decisively
— never reads OAuth credentials or the system keychain, so a subscription login stops
working.

#### Permission modes **[V]**

`default`, `acceptEdits`, `plan`, `auto`, `dontAsk`, `bypassPermissions`, `manual`.

A `-p` session starts in **Manual** on every plan. With no permission host, anything
that would prompt is denied. This is the trap described in `SPEC.md` G2 — an invocation
without an explicit mode produces prose and writes nothing.

Policy mapping (FR-CHAT-4):

| Policy | Flags |
|---|---|
| Guarded (default) | `--permission-mode acceptEdits --allowedTools "Read,Edit,Write,Glob,Grep,Bash(pio *)"` |
| Assisted | `--permission-mode auto --allowedTools "Read,Edit,Write,Glob,Grep,Bash"` |
| Unrestricted | `--dangerously-skip-permissions` |

`acceptEdits` auto-approves file writes and common filesystem commands (`mkdir`, `touch`,
`mv`, `cp`). Other shell commands still need an `--allowedTools` entry.

`--allowedTools` uses permission-rule syntax; the trailing space before `*` matters:
`Bash(pio run *)` allows `pio run -t upload` but `Bash(pio run*)` would also match
`pio runx`.

#### Session continuity **[V]**

- First turn: `--session-id <uuid>` (must be a valid UUID — generate it app-side so the
  id is known before the process starts).
- Later turns: `--resume <uuid>`. Session lookup works from any directory on the machine
  (v2.1.223+). `--resume` also accepts an absolute path to a session `.jsonl` transcript.
- `--continue` loads the most recent conversation in the cwd — **not used** by this app;
  explicit ids are required for correctness with multiple workspaces.

#### Signals **[V]**

| Signal | Behaviour |
|---|---|
| `SIGINT` | Ends the current turn cleanly. Preferred for Stop. |
| `SIGTERM` | Exits with code **143**. The in-progress turn is left unfinished with no result recorded; `SIGTERM` also kills the process tree of any running Bash command and runs `SessionEnd` hooks. Resuming continues the unfinished turn. |

Exit code 0 on success, non-zero on failure. Invalid flags are reported on **stderr before
the run starts** — this is the only reliable way to detect an unsupported flag on an older
CLI, so feature-probe by running a cheap command and checking stderr, not by version math
alone.

### 1.5 The NDJSON stream **[V]**

One JSON object per line. Parse defensively; ignore unknown `type`/`subtype`.

**Ordering.** `system/init` is the first event *unless* startup events precede it
(`plugin_install`, and `hook_started`/`hook_progress`/`hook_response` while a
`SessionStart` or `Setup` hook runs). Do not assume line 1 is `init`.

| `type` | `subtype` | Notes |
|---|---|---|
| `system` | `init` | Session metadata: model, tools, `mcp_servers[]`, `mcp_server_errors[]`, `plugins[]`, `plugin_errors[]`, `capabilities[]` (v2.1.205+ — **feature-detect with this, not version strings**), `session_id` |
| `system` | `api_retry` | `attempt`, `max_retries`, `retry_delay_ms`, `error_status`, `error` (one of `authentication_failed`, `oauth_org_not_allowed`, `account_on_hold`, `billing_error`, `rate_limit`, `overloaded`, `invalid_request`, `model_not_found`, `server_error`, `max_output_tokens`, `cloud_credential_error`, `unknown`), `uuid`, `session_id` |
| `system` | `permission_denied` | Emitted when a call is denied under `--permission-prompts none` |
| `system` | `plugin_install` | `status` ∈ `started|installed|failed|completed` |
| `assistant` | — | A complete content block. `parent_tool_use_id` is `null` for the main conversation, or the spawning tool-call id for a subagent |
| `user` | — | Tool results, and the first message of a subagent |
| `stream_event` | — | Raw Claude API streaming event in `.event`. Main session only — subagent token deltas are not forwarded |
| `result` | `success` / `error_max_turns` / … | Final line. `duration_ms`, `duration_api_ms`, `is_error`, `num_turns`, `result`, `session_id`, `total_cost_usd`, `usage`, `permission_denials[]`, and `structured_output` when `--json-schema` was used |

**Inner `stream_event.event` types:** `message_start`, `content_block_start`,
`content_block_delta`, `content_block_stop`, `message_delta`, `message_stop`.

Deltas the app cares about:
- `delta.type == "text_delta"` → `delta.text` — assistant prose.
- `delta.type == "input_json_delta"` → `delta.partial_json` — tool arguments arriving
  incrementally; accumulate per content block.

**Message ordering guarantee [V]:** with partial messages on, each `assistant` message
arrives *before* that block's `content_block_stop`:

```
stream_event(message_start)
stream_event(content_block_start)   # text
stream_event(content_block_delta)…  # text chunks
assistant                            # complete text block
stream_event(content_block_stop)
stream_event(content_block_start)   # tool_use
stream_event(content_block_delta)…  # input_json chunks
assistant                            # complete tool_use block
stream_event(content_block_stop)
…tool executes…
result
```

**Backpressure [V]:** if the consumer reads slowly, Claude Code waits for queued output
to drain before exiting, scaled to the queue, capped at 30 s. Do not let the Rust reader
block on a full IPC channel — drain into an in-memory buffer first.

**Cost [V]:** `total_cost_usd` is a client-side estimate and can differ from the bill.
Label it as an estimate in the UI.

### 1.6 Commands the app must never run

`claude` with no `-p` (interactive TUI — only ever launched in an external terminal for
auth), `claude update` (native installs self-update; package installs are the user's
business), `claude config` (would mutate the user's global settings).

---

## 2. PlatformIO Core — detection and install

### 2.1 Detection **[V]**

```
pio --version                        → "PlatformIO Core, version 6.2.0"
pio system info --json-output        → object keyed by field, each { title, value }
```

Verified `pio system info --json-output` keys: `core_version`, `python_version`,
`system`, `platform`, `filesystem_encoding`, `locale_encoding`, `core_dir`,
`platformio_exe`, `python_exe`, `global_lib_nums`, `dev_platform_nums`,
`package_tool_nums`.

```json
{
  "core_version": { "title": "PlatformIO Core", "value": "6.2.0" },
  "core_dir":     { "title": "PlatformIO Core Directory", "value": "/home/u/.platformio" },
  "platformio_exe": { "title": "PlatformIO Core Executable", "value": "/usr/local/bin/platformio" },
  "python_exe":   { "title": "Python Executable", "value": "/usr/bin/python3" },
  "dev_platform_nums": { "title": "Development Platforms", "value": 0 }
}
```

Candidate paths to probe beyond `PATH`: `~/.platformio/penv/bin/pio`,
`~/.platformio/penv/Scripts/pio.exe`, `/usr/local/bin/pio`, `/opt/homebrew/bin/pio`,
and `$PIO_CORE_DIR` if set.

Global options **[V]**: `--version`, `-c/--caller TEXT`, `--no-ansi`, `-h/--help`.
Set `--caller vibe-hardware` on every invocation so PlatformIO telemetry attributes
correctly; omit `--no-ansi` when the log pane wants colour.

### 2.2 Install **[V]**

```
curl -fsSL -o get-platformio.py \
  https://raw.githubusercontent.com/platformio/platformio-core-installer/master/get-platformio.py
python3 get-platformio.py
```

Creates a virtualenv at `<core_dir>/penv` (default `~/.platformio/penv`) and puts `pio`
there. No sudo required. If it fails, delete `penv` and re-run.

Alternative for containers/CI: `pip install platformio` (the app should offer this only
when a suitable Python with a writable environment is detected).

### 2.3 Global settings **[V]**

```
pio settings get [NAME]
pio settings set NAME VALUE
pio settings reset
```

`pio settings get` prints a table: name, current value, `[default]`, description.
Verified setting names: `check_platformio_interval`, `check_prune_system_threshold`,
`disable_udev_rules_check`, `enable_cache`, `enable_proxy_strict_ssl`,
`enable_telemetry`, `force_verbose`, `projects_dir`. There is **no** `--json-output`;
parse the table, and treat unknown rows as pass-through strings so new settings appear
automatically.

---

## 3. PlatformIO — boards and devices

### 3.1 `pio boards` **[V]**

```
pio boards [QUERY] [--installed] [--json-output]
```

`--json-output` prints a JSON **array**. Each element (from
`PlatformBoardConfig.get_brief_data`):

```jsonc
{
  "id": "esp32dev",
  "name": "Espressif ESP32 Dev Module",
  "platform": "espressif32",
  "mcu": "ESP32",                 // uppercased
  "fcpu": 240000000,              // Hz (integer)
  "ram": 327680,                  // bytes, from upload.maximum_ram_size
  "rom": 4194304,                 // bytes, from upload.maximum_size
  "frameworks": ["arduino", "espidf"],
  "vendor": "Espressif",
  "url": "https://...",
  "connectivity": ["wifi", "bluetooth", "can", "ethernet"],   // optional
  "debug": { "tools": { "esp-prog": { "default": true } } }   // optional
}
```

The free-text `QUERY` is matched against `id` plus a lowercased JSON dump of the whole
board record — so it also matches vendor, mcu, and framework. For the UI, fetch the full
list once and filter client-side (NFR-P4) rather than re-invoking per keystroke.

`--installed` restricts to platforms already installed — this is the **offline-safe**
variant. The unrestricted form requires the registry.

### 3.2 `pio device list` **[V]**

```
pio device list [--serial] [--logical] [--mdns] [--json-output]
```

`--serial` is the default when neither `--logical` nor `--mdns` is given.

**Output shape depends on how many kinds were requested.** With exactly one kind, the
JSON is that kind's **array**, unwrapped. With more than one, it is an object keyed by
kind. Always request exactly one kind so the shape is stable:

```bash
pio device list --serial --json-output
```
```json
[
  { "port": "/dev/ttyUSB0",
    "description": "CP2102 USB to UART Bridge Controller",
    "hwid": "USB VID:PID=10C4:EA60 SER=0001 LOCATION=1-1.2" }
]
```

Fields are exactly `port`, `description`, `hwid` — nothing else. Parse `VID:PID=` and
`SER=` out of `hwid` for device stickiness (FR-DEV-2). On macOS, if pyserial returns
nothing, PlatformIO falls back to globbing `/dev/tty.*` with `description` and `hwid`
both set to the literal string `"n/a"` — handle that.

### 3.3 `pio device monitor` **[V]**

```
pio device monitor
  [-p/--port TEXT] [-b/--baud INTEGER]
  [--parity N|E|O|S|M] [--rtscts] [--xonxoff]
  [--rts 0|1] [--dtr 0|1] [--echo]
  [--encoding TEXT] [-f/--filter TEXT] [--eol CR|LF|CRLF] [--raw]
  [--exit-char INTEGER] [--menu-char INTEGER]
  [--quiet] [--no-reconnect]
  [-d/--project-dir DIRECTORY] [-e/--environment TEXT]
```

Defaults: baud 9600, parity `N`, eol `CRLF`, encoding UTF-8, exit-char 3 (Ctrl+C),
menu-char 20.

**Use only for the "open in external terminal" action (FR-DEV-6).** It is an interactive
terminal program with its own menu handling; the in-app monitor uses the `serialport`
crate directly so the app controls the port lease, buffering, and reconnection.

With `-d` and `-e` it reads the env's `monitor_*` options — which is why the in-app
monitor must read the same options from `platformio.ini` to behave identically.

---

## 4. PlatformIO — project

### 4.1 `pio project init` **[V]**

```
pio project init
  [-d/--project-dir DIRECTORY]
  [-b/--board ID]                       # repeatable
  [--ide clion|codeblocks|eclipse|emacs|netbeans|qtcreator|sublimetext|vim|visualstudio|vscode]
  [-e/--environment TEXT]               # update an existing environment
  [-O/--project-option "name=value"]    # repeatable
  [--sample-code]
  [--no-install-dependencies]
  [--env-prefix TEXT]
  [-s/--silent]
```

Creates `platformio.ini`, `src/`, `include/`, `lib/`, `test/`, plus VCS/CI helper files.

App invocation:

```
pio project init -d <dir> -b <board_id> [--sample-code]
                 -O "framework=<framework>"
                 [-O "monitor_speed=115200"]
```

**Gotcha [V]:** with a board whose platform is not yet installed, this needs the registry.
Observed offline behaviour: it hangs for roughly 60 s and then fails with a bare
`HTTPClientError:` and no useful detail. Pre-check connectivity (FR-SETUP-9) and surface
a real message. `--no-install-dependencies` skips resolution but leaves a project that
cannot build until the platform is installed.

**Gotcha [V], PlatformIO Core 6.1.19:** `-d/--project-dir` is Click's `Directory` param
type and validates that the directory **already exists** — it does not create it. Passing
a not-yet-created target (the normal case for "create a new project") fails immediately
with exit code 2: `Error: Invalid value for '--project-dir' / '-d': Directory '<dir>' does
not exist.` The app must `mkdir -p` the target directory itself before invoking this
command.

First-time platform installs download toolchains of hundreds of MB. Treat as a
long-running cancellable step with explanatory text (FR-PROJ-2).

### 4.2 `pio project config` **[V]**

```
pio project config [-d/--project-dir DIRECTORY] [--lint] [--json-output]
```

`--json-output` returns the **computed** configuration — inheritance from `[env]` and
`extends` is already resolved. The shape is a nested **array**, not an object:

```json
[
  ["platformio", [["default_envs", ["esp32dev"]]]],
  ["env",        [["monitor_speed", 115200]]],
  ["env:esp32dev", [
      ["platform", "espressif32"],
      ["board", "esp32dev"],
      ["framework", ["arduino"]],
      ["build_flags", ["-DDEBUG=1", "-Wall"]],
      ["lib_deps", ["bblanchon/ArduinoJson@^7.0.0"]],
      ["monitor_speed", 115200]            // ← inherited from [env]
  ]]
]
```

Note the value types: `multiple` options come back as arrays, integers as numbers. This
is the source for the **Effective** column (FR-INI-3). It is **not** a source for editing
— it does not distinguish declared from inherited, and it drops comments.

### 4.3 `pio project config --lint --json-output` — **known defect [V]**

The source is:

```python
result = ProjectConfig.lint()          # {"errors": [...], "warnings": [...]}
if json_output:
    return click.echo(result)          # ← prints a Python repr, NOT json.dumps
```

Observed output: `{'errors': [], 'warnings': []}` — **single quotes, invalid JSON.**
Do not call `JSON.parse` on it. Either run the command **without** `--json-output` and
parse the human table, or parse the repr defensively (a `'` → `"` substitution is
unsafe in general because messages may contain apostrophes). Recommended: omit
`--json-output` and parse the tabulated form, where errors carry `type`, `message`, and
`source:lineno`, and warnings are plain strings.

### 4.4 `pio project metadata` **[V]**

```
pio project metadata [-d/--project-dir DIRECTORY] [-e/--environment TEXT]
                     [--json-output] [--json-output-path PATH]
```

Emits IDE-oriented build metadata keyed by environment name: include paths, defines,
compiler paths, the build target, and more. Useful for the read-only source viewer and
for grounding `CLAUDE.md`.

**Gotcha [V]:** without `--json-output`, this command *installs project dependencies* as
a side effect. Always pass `--json-output` when you only want to read.

---

## 5. PlatformIO — build, upload, analyse, test

### 5.1 `pio run` **[V]**

```
pio run
  [-e/--environment TEXT]   # repeatable
  [-t/--target TEXT]        # repeatable
  [--upload-port TEXT] [--monitor-port TEXT] [-p/--port TEXT]
  [-d/--project-dir PATH] [-c/--project-conf FILE]
  [-j/--jobs INTEGER] [-a/--program-arg TEXT]
  [--disable-auto-clean] [--list-targets] [-s/--silent] [-v/--verbose]
```

App invocations:

| Action | argv |
|---|---|
| Build | `pio run -d <dir> -e <env>` |
| Upload | `pio run -d <dir> -e <env> -t upload --upload-port <port>` |
| Size | `pio run -d <dir> -e <env> -t size` |
| Clean | `pio run -d <dir> -e <env> -t clean` |
| Full clean | `pio run -d <dir> -e <env> -t fullclean` |
| Filesystem image | `pio run -d <dir> -e <env> -t buildfs` / `-t uploadfs` **[U]** — platform-provided |
| Erase flash | `pio run -d <dir> -e <env> -t erase` **[U]** — platform-provided |
| compile_commands.json | `pio run -d <dir> -e <env> -t compiledb` **[U]** |

**`--list-targets` has no `--json-output` [V].** It prints a table. Worse, it requires the
platform to be installed — offline with an uninstalled platform it hangs on registry
calls. Therefore: show a hardcoded universal subset (`build`, `upload`, `clean`,
`fullclean`, `size`, `monitor`) until a successful `--list-targets` has been cached per
environment (FR-BUILD-7).

**Cancellation [V]:** SCons spawns compilers as grandchildren. Killing the `pio` PID
leaves them running. Terminate the process group / job object.

**Output parsing.** Two parsers over the combined stream:

1. *Diagnostics* —
   `^(?<file>[^\s:]+):(?<line>\d+):(?:(?<col>\d+):)?\s+(?<sev>error|warning|note|fatal error):\s+(?<msg>.*)$`
   plus the linker form `undefined reference to \`(?<sym>.+)'` and the preceding
   `(?<file>.+):(?<line>\d+):` line.
2. *Size* — the `RAM:   [====      ]  38.5% (used 126112 bytes from 327680 bytes)` and
   `Flash: ...` lines. Capture percent, used, and total for both.

Terminal status lines to key the UI off: `SUCCESS`/`FAILED` in the environment summary
table at the end of a run, and the `Building .pio/build/<env>/firmware.bin` /
`Writing at 0x...` progress lines during upload.

### 5.2 `pio check` **[V]**

```
pio check [-e TEXT] [-d PATH] [-c FILE] [-f/--src-filters TEXT] [--flags TEXT]
          [--severity low|medium|high] [-s] [-v] [--json-output]
          [--fail-on-defect low|medium|high] [--skip-packages]
```

`--json-output` is supported here. Feed results into the same Problems list as build
diagnostics.

### 5.3 `pio test` **[V]**

```
pio test [-e TEXT] [-f/--filter PATTERN] [-i/--ignore PATTERN]
         [--upload-port TEXT] [--test-port TEXT] [-d DIRECTORY] [-c FILE]
         [--without-building] [--without-uploading] [--without-testing] [--no-reset]
         [--monitor-rts 0|1] [--monitor-dtr 0|1] [-a TEXT]
         [--list-tests] [--json-output] [--json-output-path PATH]
         [--junit-output-path PATH] [-v|-vv|-vvv]
```

Note `--test-port` is separate from `--upload-port`, and running tests takes the serial
port — it must go through the `PortBroker`.

---

## 6. PlatformIO — the `platformio.ini` option schema

There is **no CLI command** that dumps the option schema. The app extracts it by running
a one-liner in PlatformIO's own Python environment. `python_exe` comes from
`pio system info --json-output`:

```
<python_exe> -c "import json;from platformio.project.options import ProjectOptions;print(json.dumps({k:v.as_dict() for k,v in ProjectOptions.items()}))"
```

**[V]** Verified on Core 6.2.0: 84 options. Keys are `"<scope>.<name>"`, e.g.
`"env.build_flags"`. Each value is `ConfigOption.as_dict()`:

```json
{
  "scope": "env", "group": "build", "name": "build_flags",
  "description": "Custom build flags/options for preprocessing, compilation, assembly, and linking processes",
  "type": "string", "multiple": true,
  "sysenvvar": "PLATFORMIO_BUILD_FLAGS", "default": null
}
```

`type` is `"string"` by default, or a click param-type name (`"integer"`, `"choice"`,
`"path"`, `"boolean"`, `"float"`). Extra keys appear conditionally:
`choices` for `choice`, and `min`/`max` for ranges.

Verified samples:

```json
{"scope":"env","group":"monitor","name":"monitor_speed","description":"A monitor speed (baud rate)","type":"integer","multiple":false,"sysenvvar":null,"default":9600}
{"scope":"env","group":"library","name":"lib_ldf_mode","description":"Control how Library Dependency Finder (LDF) should analyze dependencies (`#include` directives)","type":"choice","multiple":false,"sysenvvar":null,"default":"chain","choices":["off","chain","deep","chain+","deep+"]}
{"scope":"env","group":"build","name":"build_type","description":"Project build configuration","type":"choice","multiple":false,"sysenvvar":null,"default":"release","choices":["release","test","debug"]}
{"scope":"env","group":"check","name":"check_severity","description":"List of defect severity types for analysis","type":"choice","multiple":true,"sysenvvar":null,"default":["low","medium","high"],"choices":["low","medium","high"]}
```

### Verified groups and option names (Core 6.2.0)

| Scope | Group | Options |
|---|---|---|
| `platformio` | `generic` | `name`, `description`, `default_envs`, `extra_configs` |
| `platformio` | `directory` | `core_dir`, `globallib_dir`, `platforms_dir`, `packages_dir`, `cache_dir`, `build_cache_dir`, `workspace_dir`, `build_dir`, `libdeps_dir`, `include_dir`, `src_dir`, `lib_dir`, `data_dir`, `test_dir`, `boards_dir`, `monitor_dir`, `shared_dir` |
| `env` | `platform` | `platform`, `platform_packages`, `board`, `framework`, `board_build.mcu`, `board_build.f_cpu`, `board_build.f_flash`, `board_build.flash_mode` |
| `env` | `build` | `build_type`, `build_flags`, `build_src_flags`, `build_unflags`, `build_src_filter`, `targets` |
| `env` | `upload` | `upload_port`, `upload_protocol`, `upload_speed`, `upload_flags`, `upload_resetmethod`, `upload_command` |
| `env` | `monitor` | `monitor_port`, `monitor_speed`, `monitor_parity`, `monitor_filters`, `monitor_rts`, `monitor_dtr`, `monitor_eol`, `monitor_raw`, `monitor_echo`, `monitor_encoding` |
| `env` | `library` | `lib_deps`, `lib_ignore`, `lib_extra_dirs`, `lib_ldf_mode`, `lib_compat_mode`, `lib_archive` |
| `env` | `check` | `check_tool`, `check_src_filters`, `check_flags`, `check_severity`, `check_skip_packages` |
| `env` | `test` | `test_framework`, `test_filter`, `test_ignore`, `test_port`, `test_speed`, `test_build_src`, `test_testing_command` |
| `env` | `debug` | `debug_tool`, `debug_build_flags`, `debug_init_break`, `debug_init_cmds`, `debug_extra_cmds`, `debug_load_cmds`, `debug_load_mode`, `debug_server`, `debug_port`, `debug_speed`, `debug_svd_path`, `debug_server_ready_pattern`, `debug_test` |
| `env` | `advanced` | `extends`, `extra_scripts` |

Cache the extracted schema keyed by `core_version`. Re-extract when the version changes.
Ship a bundled fallback copy so the form still renders if extraction fails.

### 6.1 Other useful in-process lookups **[V]**

The same `python_exe` trick resolves things the CLI does not expose:

```
# canonical udev rules file that ships with this Core version (Linux)
<python_exe> -c "from platformio.fs import get_platformio_udev_rules_path as p;print(p())"
```

Keep this list short and treat each entry as a private API: guard every call, and fall
back to a bundled default if the import fails after a PlatformIO upgrade.

---

## 7. PlatformIO — packages and libraries

### 7.1 `pio pkg` subcommands **[V]**

`exec`, `install`, `list`, `outdated`, `pack`, `publish`, `search`, `show`,
`uninstall`, `unpublish`, `update`.

### 7.2 `pio pkg install` **[V]**

```
pio pkg install
  [-d/--project-dir DIRECTORY] [-e/--environment TEXT]
  [-p/--platform SPEC] [-t/--tool SPEC] [-l/--library SPEC]
  [--no-save] [--skip-dependencies] [-g/--global] [--storage-dir DIRECTORY]
  [-f/--force] [-s/--silent]
```

Installing a library **writes `lib_deps` for you** unless `--no-save` is given — so the
Libraries UI should not also patch the ini (double entry). `pio pkg uninstall` takes the
same options.

### 7.3 `pio pkg list` / `outdated` **[V]**

```
pio pkg list [-d DIRECTORY] [-e TEXT] [-p SPEC] [-t SPEC] [-l SPEC]
             [-g] [--storage-dir DIRECTORY]
             [--only-platforms] [--only-tools] [--only-libraries] [-v]
pio pkg outdated [-d DIRECTORY] [-e TEXT]
```

**Neither supports `--json-output` in Core 6.2.0.** Parse the tree-style text output, and
treat unparseable lines as informational rather than failing.

### 7.4 `pio pkg search` — **no JSON output [V]**

```
pio pkg search QUERY [-p/--page INT] [-s/--sort relevance|popularity|trending|added|updated]
```

Human-readable only. For the library browser, call the registry HTTP API that this
command itself uses:

```
GET https://api.registry.platformio.org/v3/search?query=<q>[&page=<n>][&sort=<s>]
Mirror: https://api.registry.nm1.platformio.org/v3/search
Package detail: /v3/packages/{owner}/{type}/{name}[?version=]
```

Qualifier syntax inside `query` (verified against the client source): `author:"..."`,
`keyword:"..."`, `framework:"..."`, `platform:"..."`, `header:"..."`, `id:"..."`,
`name:"..."`, `owner:"..."`, `type:"..."` — note the qualifier is the **singular** of the
parameter name. Multiple qualifiers are space-joined with the free-text query.

Response shape (from the CLI's own consumption): `{ total, page, limit, items: [...] }`
where each item has `owner.username`, `name`, `type`, `tier`, `description`, and
`version: { name, released_at }`.

**[U]** The full response schema is not documented; code defensively and treat any
missing field as absent rather than erroring.

### 7.5 `pio pkg exec` **[V]**

```
pio pkg exec [-p/--package SPEC] [-c/--call <cmd> [args...]] [ARGS...]
```

Runs a command from an installed tool package. This is how ESP telemetry is obtained:

```
pio pkg exec -p "tool-esptoolpy" -- esptool.py flash_id
```

**Gotcha [U]:** the entrypoint name is not stable across `tool-esptoolpy` versions —
newer esptool releases install as `esptool` rather than `esptool.py`. The telemetry
adapter must try `esptool.py`, fall back to `esptool`, and cache whichever succeeded per
installed package version. Also note that output is prefixed with a line like
`Using tool-esptoolpy@1.40501.0 package` before the tool's own output — strip it.

The port must be leased before this runs, and released after.

### 7.6 `pio system prune` **[V]**

```
pio system prune
```

Removes unused data (caches, unused packages). Expose in Advanced settings with a
confirmation showing what will be freed.

---

## 8. Git

Used only through `core/snapshot`. Prefer `git2` (libgit2) so a missing `git` binary is
not fatal; keep a CLI fallback for environments where libgit2 misbehaves.

```
git --git-dir=<ws>/.vibe/shadow --work-tree=<ws> init
git --git-dir=<ws>/.vibe/shadow --work-tree=<ws> add -A
git --git-dir=<ws>/.vibe/shadow --work-tree=<ws> commit -m "pre-turn <id>" --allow-empty
git --git-dir=<ws>/.vibe/shadow --work-tree=<ws> diff --name-status <id> --
git --git-dir=<ws>/.vibe/shadow --work-tree=<ws> checkout <id> -- <path>
```

`.vibe/` goes into `<ws>/.vibe/shadow/info/exclude`. If the workspace already has its own
git repo, use `refs/vibe/snapshots/<turn_id>` via plumbing instead, so HEAD, the index,
and the user's branches are never touched.

---

## 9. Opening the user's editor

Resolution order (FR-PROJ-6): explicit setting → `$VISUAL` → `$EDITOR` → first of
`code`, `cursor`, `zed`, `subl`, `idea`, `nvim` found on PATH → OS default handler.

```
<editor> <workspace_dir>
<editor> --goto <file>:<line>          # VS Code family, for "open at this line"
```

Reveal in file manager uses the `opener` plugin rather than a spawned process.

---

## 10. Quick reference — commands by feature

| Feature | Command(s) |
|---|---|
| Doctor | `claude --version`, `claude doctor`, `pio --version`, `pio system info --json-output` |
| Create project | `pio project init -d … -b … -O framework=…` |
| Board picker | `pio boards --json-output` (cached), `pio boards --installed --json-output` (offline) |
| Device list | `pio device list --serial --json-output` |
| ESP telemetry | `pio pkg exec -p "tool-esptoolpy" -- esptool.py flash_id` |
| Build | `pio run -d … -e …` |
| Upload | `pio run -d … -e … -t upload --upload-port …` |
| Extra targets | `pio run -d … -e … --list-targets` (text, cached) |
| Static analysis | `pio check … --json-output` |
| Tests | `pio test … --json-output` |
| Effective config | `pio project config -d … --json-output` |
| Lint config | `pio project config -d … --lint` (**not** `--json-output`) |
| Option schema | `<python_exe> -c "…ProjectOptions…"` |
| Library search | `GET api.registry.platformio.org/v3/search?query=…` |
| Library install | `pio pkg install -d … -e … -l "owner/name@^1.2.3"` |
| Installed packages | `pio pkg list -d … -e …`, `pio pkg outdated -d … -e …` |
| Global PIO settings | `pio settings get`, `pio settings set NAME VALUE` |
| Claude turn | `claude -p "…" --output-format stream-json --verbose --include-partial-messages --permission-mode acceptEdits --allowedTools "…" --session-id/--resume <uuid>` |
| External monitor | `pio device monitor -d … -e …` |
