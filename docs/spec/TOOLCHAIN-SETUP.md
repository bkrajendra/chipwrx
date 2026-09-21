# TOOLCHAIN-SETUP.md — Doctor, auto-install, and auth

Closes gaps **G1, G3, G14, G17, G23**. This is the difference between a demo and a
product: a user who has never opened a terminal must reach a blinking LED.

---

## 1. Design rules

1. **Probe, never assume.** No code path assumes `pio` or `claude` is on `PATH`.
2. **Show the command.** Every install action displays the exact argv and its source URL
   before running. The app never pipes a downloaded script into a shell invisibly.
3. **Never escalate.** The app does not run `sudo`. Where privileges are required (Linux
   udev rules, some driver installs), it shows a copyable command block and a "I've run
   it, re-check" button.
4. **Adopt before installing.** If a working install exists anywhere, offer to use it.
   Fay must never end up with two PlatformIO installs.
5. **Non-blocking.** Doctor runs concurrently in the background. The app is usable —
   browse projects, read transcripts, edit `platformio.ini` — while probes are pending.
6. **Idempotent.** Re-running any install action on an already-healthy system is safe and
   reports "already installed".

---

## 2. Probe matrix

Each probe is independent, timeout-bounded, and resolves to
`Ok | Missing | Degraded | Error` (see `IPC-CONTRACT.md` §2).

| Probe | Method | Timeout | Failure meaning |
|---|---|---|---|
| `claudeBinary` | resolve path → `claude --version` → parse `^(\d+\.\d+\.\d+)` | 5 s | Missing → offer install |
| `claudeAuth` | a minimal `claude -p` probe turn; inspect `result.is_error` and any `api_retry.error` | 30 s | `authentication_failed` / `account_on_hold` / `billing_error` are distinct states with distinct copy |
| `claudeCapabilities` | `capabilities[]` from `system/init` of the probe turn | — | Absent on < 2.1.205; fall back to version comparison |
| `pioBinary` | resolve path → `pio --version` | 5 s | Missing → offer install |
| `pioCoreDir` | `pio system info --json-output` → `core_dir`; check exists + writable | 10 s | Degraded if read-only (common on locked-down corporate images) |
| `python` | probe `python3`, `python`, `py -3`; require ≥ 3.6 | 5 s | Only needed if PIO must be installed |
| `networkRegistry` | `HEAD https://api.registry.platformio.org/v3/search?query=test` | 20 s | Degraded → offline mode banner |
| `serialPermissions` | Linux: can the user read a `/dev/tty*` device, or is the `dialout`/`uucp` group present? macOS/Windows: always Ok, with driver hints surfaced on device-open failure | 3 s | Degraded → udev remediation |
| `git` | `git --version`, or libgit2 availability | 3 s | Degraded → snapshots use libgit2 only |

Doctor re-runs: on launch, on explicit request, after any install action, and on window
focus if the last run is older than 6 hours.

---

## 3. Binary resolution

The macOS `.app` launch environment does not include a login shell's `PATH`. This is the
single most common "it works in my terminal" bug in GUI developer tools. Resolution order:

1. `settings.toolchain.<tool>Path` if set and still executable.
2. The process `PATH`.
3. A per-OS candidate list:

| Tool | macOS / Linux | Windows |
|---|---|---|
| `claude` | `~/.local/bin/claude`, `/opt/homebrew/bin/claude`, `/usr/local/bin/claude`, `/usr/bin/claude` | `%USERPROFILE%\.local\bin\claude.exe`, `%LOCALAPPDATA%\Programs\claude\claude.exe` |
| `pio` | `~/.platformio/penv/bin/pio`, `/opt/homebrew/bin/pio`, `/usr/local/bin/pio`, `$PIO_CORE_DIR/penv/bin/pio` | `%USERPROFILE%\.platformio\penv\Scripts\pio.exe` |
| `python3` | `/opt/homebrew/bin/python3`, `/usr/local/bin/python3`, `/usr/bin/python3` | `py -3`, `%LOCALAPPDATA%\Programs\Python\Python3*\python.exe` |

4. Last resort on macOS/Linux: run the user's login shell non-interactively
   (`$SHELL -lc 'command -v claude'`) and take the result. Do this **once**, cache it, and
   never on a hot path.

Store the resolved absolute path in settings and use it for every invocation thereafter.
Re-resolve when the recorded path stops being executable.

---

## 4. Claude Code: install

### 4.1 Commands offered per platform

| Platform | Primary | Alternatives shown |
|---|---|---|
| macOS | `curl -fsSL https://claude.ai/install.sh \| bash` | `brew install --cask claude-code` |
| Linux / WSL | `curl -fsSL https://claude.ai/install.sh \| bash` | apt / dnf / apk repositories, `npm install -g @anthropic-ai/claude-code` |
| Windows | `irm https://claude.ai/install.ps1 \| iex` (PowerShell) | `winget install Anthropic.ClaudeCode`, CMD variant |

The app runs the primary command through `ProcessSupervisor` with output streamed to an
install log pane, then re-probes. If the app detects an existing Homebrew/WinGet/apt
install, it prefers the matching upgrade command over the native installer so the user
does not end up with two.

### 4.2 UI flow

```
[ Claude Code — not found ]
The app needs the Claude Code CLI to generate firmware.

It will run:
  curl -fsSL https://claude.ai/install.sh | bash
Source: https://claude.ai/install.sh   ·   Docs: code.claude.com/docs/en/setup

[ Install ]  [ I'll install it myself ]  [ Choose an existing installation… ]
```

Live output goes to a collapsible log. On completion, re-probe; on failure, show the last
20 lines with a "Copy command" button so the user can run it in their own terminal.

### 4.3 Version floor

Minimum **2.1.0**. Below that, block turns and offer an update. Feature gates:

| Feature | Requirement | Fallback |
|---|---|---|
| `--permission-prompts none` | 2.1.259+ | Omit the flag; rely on permission mode |
| `capabilities[]` on `system/init` | 2.1.205+ | Version comparison |
| Nested subagent messages in stream | 2.1.219+ | Attribute only top-level subagents |
| `--resume` finds a session from any directory | 2.1.223+ | Always spawn in the workspace dir (which the app does anyway) |

Feature-detect from `capabilities[]` first; use version numbers only when no capability
string covers the feature.

---

## 5. Claude Code: authentication

There is **no headless login**. Claude Code requires a Pro, Max, Team, Enterprise, or
Console account (the free claude.ai plan does not include Claude Code).

### 5.1 Detection

Run a probe turn:

```
claude -p "reply with the single word: ok"
  --output-format stream-json --verbose
  --permission-mode dontAsk
  --max-turns 1
```

| Observation | Interpretation |
|---|---|
| `result` with `is_error: false` | Authenticated |
| `api_retry` with `error: "authentication_failed"` | Not logged in |
| `api_retry` with `error: "account_on_hold"` / `"billing_error"` | Account problem — distinct copy, link to account settings |
| `api_retry` with `error: "oauth_org_not_allowed"` | Org policy blocks this account |
| Non-zero exit, no `result` | Treat as `Error` with the captured stderr |

Note: a failure inside the run is printed as the **result on stdout**, not as a
stderr argument error. Do not look only at stderr.

Cache the result; re-probe on launch and after any auth action. Never probe on every turn.

### 5.2 Remediation

```
[ Claude Code — not signed in ]
Sign in once and the app will use that session.

[ Open a terminal and sign in ]     ← launches `claude` in the OS terminal
[ Use a long-lived token ]          ← runs `claude setup-token`, same way
[ Use an API key instead ]          ← stores ANTHROPIC_API_KEY in the OS keychain
[ Re-check ]
```

Terminal launch per OS:

| OS | Approach |
|---|---|
| macOS | `open -a Terminal <script>` where the script runs the resolved `claude` |
| Windows | `cmd /c start "" cmd /k "<claude path>"` |
| Linux | First available of `x-terminal-emulator`, `gnome-terminal`, `konsole`, `xfce4-terminal`, `alacritty`, `kitty`; if none, show the command to paste |

The API-key path (NFR-S1): stored in the OS keychain via `keyring`, injected into the
child environment only, redacted from every log sink. The app states plainly that a
Console API key is billed per token, unlike a subscription.

---

## 6. PlatformIO Core: install

### 6.1 Adopt first

Scan the candidate paths in §3. If a `pio` is found, show:

```
Found PlatformIO Core 6.2.0 at ~/.platformio/penv/bin/pio
[ Use this ]   [ Install a separate copy for this app ]
```

Adoption is the default. This matters for Fay, whose VS Code extension and CI both use
that install.

### 6.2 Install

Requires Python 3 (§2). The app does **not** install Python — it names the version needed
and links to python.org / the platform package manager.

```
curl -fsSL -o get-platformio.py \
  https://raw.githubusercontent.com/platformio/platformio-core-installer/master/get-platformio.py
<python> get-platformio.py
```

- Creates a virtualenv at `<core_dir>/penv`, default `~/.platformio/penv`.
- No administrative privileges required.
- On failure, the documented recovery is to delete `penv` and re-run — the app offers
  this as a "Repair installation" action.
- Progress is streamed; the toolchain download is large, so the pane shows elapsed time
  and a plain-English note about size.

After installation, re-resolve the binary (it will not be on `PATH`) and record the
absolute path.

### 6.3 Version floor and health

Minimum Core **6.1.0**; verified against **6.2.0**. Read `core_version` from
`pio system info --json-output`. Below the floor: offer `pio upgrade` (native/pip
installs) or the package-manager equivalent, and block project creation.

Also check from the same output:
- `core_dir` exists and is writable → otherwise `Degraded` with a clear explanation
  (corporate-managed home directories are the usual cause).
- `dev_platform_nums == 0` → informational: "No platforms installed yet; the first
  project will download one."

---

## 7. Serial permissions and drivers

### 7.1 Linux — udev rules (G17)

Symptom: `pio device list` shows the port but opening it fails with permission denied.

**Detection — mirror PlatformIO's own logic [V]** (`platformio/fs.py::ensure_udev_rules`),
rather than inventing one:

1. Linux only, and skipped entirely when the `disable_udev_rules_check` setting is on.
2. Rules are considered installed if either
   `/etc/udev/rules.d/99-platformio-udev.rules` or
   `/lib/udev/rules.d/99-platformio-udev.rules` exists. Neither → **missing**.
3. If present, compare against PlatformIO's canonical copy, which ships **inside the
   installed package** at `<pio_pkg>/assets/system/99-platformio-udev.rules`. Read both,
   ignoring blank lines and `#` comments, and treat the installed file as **outdated**
   if the canonical rule set is not a subset of it.
4. Independently, attempt a read-only open of the selected port; on `EACCES`, check
   whether the user is in `dialout` (Debian/Ubuntu) or `uucp` (Arch/Fedora).

Resolve the canonical path from `pio system info --json-output` → `python_exe`:

```
<python_exe> -c "from platformio.fs import get_platformio_udev_rules_path as p;print(p())"
```

**Copy from that local file** rather than downloading — it matches the installed Core
version exactly, and it works offline.

Remediation — shown as a copyable block, never executed by the app:

```bash
sudo cp "<canonical path from above>" /etc/udev/rules.d/99-platformio-udev.rules
sudo udevadm control --reload-rules && sudo udevadm trigger   # or: sudo service udev restart
sudo usermod -a -G dialout "$USER"   # use uucp on Arch/Fedora
# then log out and back in, and re-plug the board
```

Followed by a **[ Re-check ]** button. The "log out and back in" step is the part users
miss — call it out explicitly.

PlatformIO's own errors for these two states are `MissedUdevRules` and
`OutdatedUdevRules`; both point at
`https://docs.platformio.org/en/latest/core/installation/udev-rules.html`, which the app
should link rather than paraphrase. The `disable_udev_rules_check` global setting is
exposed in the PlatformIO settings screen for users who manage rules themselves.

### 7.2 macOS

Modern macOS includes CP210x and CH34x/CH9102 support, but many cheap boards still ship
with a bridge needing a vendor kext/DEXT. Detection is by absence: the device enumerates
under a different name, or not at all. Remediation is informational — identify the bridge
from the USB `hwid` VID:PID and link to the vendor driver, with a warning that a reboot
may be required.

Known VID:PID → bridge mapping to ship in the app:

| VID:PID | Bridge |
|---|---|
| `10C4:EA60` | Silicon Labs CP2102/CP2109 |
| `1A86:7523` | WCH CH340/CH341 |
| `1A86:55D4` | WCH CH9102 |
| `0403:6001` / `0403:6015` | FTDI FT232R / FT231X |
| `303A:*` | Espressif native USB (ESP32-S2/S3/C3) |
| `2E8A:*` | Raspberry Pi RP2040 |

Use it for FR-DEV-9 toasts and for driver hints.

### 7.3 Windows

Same bridge mapping. When a device is present in `pio device list` but cannot be opened,
the usual causes are a missing driver, a port held by another program (a stray PuTTY or
Arduino IDE), or exclusive access. Message accordingly, naming the port, and offer
"Re-scan".

---

## 8. Network and offline behaviour (G14)

Pre-flight `networkRegistry` before any action that needs the registry: project creation
with a platform that is not installed, board catalogue refresh, library search, library
install, and `pio run --list-targets` for an uninstalled platform.

Observed offline behaviour of `pio project init -b <board>` on a machine that cannot
reach the registry: it hangs for roughly 60 s, then fails with a bare `HTTPClientError:`
and no detail. Never let the user see that. Instead:

```
[ PlatformIO's package registry is unreachable ]
Creating a project for a new board needs to download its toolchain.

You can still:
 · open existing projects
 · build with platforms already installed
 · connect to a board and use the serial monitor

[ Retry ]   [ Work offline ]   [ Check proxy settings ]
```

If the machine is behind a proxy, surface `HTTP_PROXY`/`HTTPS_PROXY` detection and
PlatformIO's own `enable_proxy_strict_ssl` setting in the advanced panel.

Offline fallbacks:
- Board picker → `pio boards --installed --json-output`, with a banner.
- Targets menu → hardcoded universal subset.
- Library browser → "unavailable offline", installed list still works.

---

## 9. First-run onboarding (FR-SETUP-8)

Four steps, each skippable, each re-enterable from Doctor.

```
① Toolchain      Claude Code ✓ 2.1.211     PlatformIO ✗  [ Install ]
② Sign in        Claude Code — not signed in  [ Sign in ]
③ Project        [ Create a project ]  [ Open an existing folder ]
④ Board          Plug in your board — we'll detect it.
                 Detected: /dev/cu.usbserial-0001 (CP2102)  [ Use this ]
```

Design notes:
- Step ① and ② run concurrently with the user reading step ③; nothing is serialised
  unnecessarily.
- The user can reach step ③ with a red toolchain — they just cannot build yet, and the
  Build button explains why.
- Completion state is per-machine, in `settings.json`, not per-project.

---

## 10. Diagnostics bundle (NFR-R1)

One action, one zip, for support:

```
vibe-hardware-diagnostics-<timestamp>.zip
├─ app.log                     # rotated logs, secrets redacted
├─ doctor.json                 # the full DoctorReport
├─ pio-system-info.json        # pio system info --json-output
├─ pio-settings.txt            # pio settings get
├─ claude-doctor.txt           # claude doctor
├─ app-settings.json           # GlobalSettings, paths redacted to ~
├─ platformio.ini              # active workspace, verbatim
├─ last-build.log              # tail of the most recent pio run
└─ env.txt                     # PATH, proxy vars, OS/arch, app version + git SHA
```

The redaction filter (`sk-ant-…`, `ANTHROPIC_API_KEY=…`, bearer tokens, `SER=` values if
the user opts in) runs over every text member before the zip is written. The app shows
the file list and offers "Reveal in Finder/Explorer" rather than uploading anything.
