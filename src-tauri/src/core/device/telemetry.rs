//! Board-family telemetry adapters (`FR-DEV-3`, `CLI-CONTRACT.md` §7.5). A missing or
//! failing adapter renders `unavailable_reason` — never an `AppError`, and never blocks
//! the UI, per the FR's own wording.
//!
//! The ESP adapter's parsing is grounded in two real captures against a physical
//! ESP32-C6-DevKitM-1 (CP210x bridge on COM10): the connect banner
//! (`Chip type:`/`MAC:`) from M5's upload transcript
//! (`tests/fixtures/pio-run-upload-success.txt`), and the `flash_id`-specific
//! `Manufacturer:`/`Detected flash size:` lines from a real `flash_id` run captured this
//! session (`tests/fixtures/esptool-flash-id-real.txt`) — `SPEC.md` §8 open question 24 is
//! resolved. That same run also confirmed open question 23's concern was real: omitting
//! `--port` makes esptool probe *every* serial port on the system (including unrelated
//! Bluetooth COM ports), which is slow and noisy — so this adapter passes `--port` to
//! esptool explicitly. That's esptool's own flag, not an invented `pio` one; `CLI-CONTRACT.md`
//! §7.5's example simply didn't need to show every arg after `--`, since everything past it
//! is forwarded to the sub-command verbatim.

use crate::core::pio::boards::BoardBrief;
use crate::core::proc::{ProcKind, ProcessSupervisor, SpawnSpec};
use crate::core::toolchain::resolve::Resolution;
use regex::Regex;
use serde::Serialize;
use std::path::Path;
use std::sync::LazyLock;
use std::sync::Mutex;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Telemetry {
    pub adapter: String,
    pub connected: bool,
    pub port: Option<String>,
    pub chip: Option<String>,
    pub flash_size: Option<String>,
    pub flash_vendor: Option<String>,
    pub mac: Option<String>,
    pub board: Option<BoardBrief>,
    pub unavailable_reason: Option<String>,
}

/// Caches which esptool entrypoint name worked ("esptool.py" vs "esptool" —
/// `CLI-CONTRACT.md` §7.5's documented gotcha) for the lifetime of the app, so every
/// telemetry refresh after the first doesn't re-probe both names.
#[derive(Default)]
pub struct EsptoolEntrypointCache(Mutex<Option<&'static str>>);

impl EsptoolEntrypointCache {
    pub fn new() -> Self {
        Self::default()
    }
}

pub fn is_esp_platform(platform: &str) -> bool {
    platform.to_ascii_lowercase().starts_with("espressif")
}

static USING_PACKAGE_PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^Using [^\n]*package\n?").unwrap());
static CHIP_TYPE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Chip type:\s+(.+)").unwrap());
static MAC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"MAC:\s+([0-9a-fA-F:]+)").unwrap());
static FLASH_SIZE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)detected flash size:\s+(\S+)").unwrap());
static FLASH_MANUFACTURER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)manufacturer:\s+(\S+)").unwrap());

fn strip_package_prefix(output: &str) -> &str {
    match USING_PACKAGE_PREFIX.find(output) {
        Some(m) if m.start() == 0 => &output[m.end()..],
        _ => output,
    }
}

struct ParsedFlashId {
    chip: Option<String>,
    mac: Option<String>,
    flash_size: Option<String>,
    flash_vendor: Option<String>,
}

fn parse_flash_id_output(raw: &str) -> ParsedFlashId {
    let text = strip_package_prefix(raw);
    ParsedFlashId {
        chip: CHIP_TYPE.captures(text).map(|c| c[1].trim().to_string()),
        mac: MAC.captures(text).map(|c| c[1].to_string()),
        flash_size: FLASH_SIZE.captures(text).map(|c| c[1].to_string()),
        flash_vendor: FLASH_MANUFACTURER.captures(text).map(|c| c[1].to_string()),
    }
}

/// `CLI-CONTRACT.md` §7.5's `pio pkg exec -p "tool-esptoolpy" -- esptool.py flash_id`, with
/// `--port` added — esptool's own flag, forwarded verbatim after `--`, not an invented
/// `pio` one. Confirmed against a real ESP32-C6-DevKitM-1 this session: omitting `--port`
/// makes esptool probe every serial port on the system (slow, noisy, and ambiguous with
/// more than one board attached) — `SPEC.md` §8 open question 23.
fn flash_id_args(entrypoint: &str, port: &str) -> Vec<String> {
    ["pkg", "exec", "-p", "tool-esptoolpy", "--", entrypoint, "--port", port, "flash_id"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

async fn try_entrypoint(
    supervisor: &ProcessSupervisor,
    pio: &Resolution,
    cwd: &Path,
    entrypoint: &str,
    port: &str,
) -> Option<String> {
    let mut args = pio.extra_args.clone();
    args.extend(flash_id_args(entrypoint, port));
    let out = supervisor
        .spawn_capture(SpawnSpec {
            program: pio.program.clone(),
            args,
            cwd: cwd.to_path_buf(),
            env: vec![],
            kind: ProcKind::Pio,
            label: "pio-pkg-exec-esptool-flash-id".into(),
        })
        .await
        .ok()?;
    if out.exit_code != 0 {
        return None;
    }
    Some(format!("{}{}", out.stdout, out.stderr))
}

async fn run_esp_adapter(
    supervisor: &ProcessSupervisor,
    pio: &Resolution,
    cwd: &Path,
    cache: &EsptoolEntrypointCache,
    port: &str,
    board: Option<BoardBrief>,
) -> Telemetry {
    let cached_entrypoint = *cache.0.lock().unwrap_or_else(|e| e.into_inner());

    let entrypoints: &[&str] = match cached_entrypoint {
        Some(e) => &[e],
        None => &["esptool.py", "esptool"],
    };

    for entrypoint in entrypoints {
        if let Some(output) = try_entrypoint(supervisor, pio, cwd, entrypoint, port).await {
            if cached_entrypoint.is_none() {
                let leaked: &'static str = if *entrypoint == "esptool.py" { "esptool.py" } else { "esptool" };
                *cache.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(leaked);
            }
            let parsed = parse_flash_id_output(&output);
            return Telemetry {
                adapter: "esp".into(),
                connected: true,
                port: Some(port.to_string()),
                chip: parsed.chip,
                flash_size: parsed.flash_size,
                flash_vendor: parsed.flash_vendor,
                mac: parsed.mac,
                board,
                unavailable_reason: None,
            };
        }
    }

    Telemetry {
        adapter: "esp".into(),
        connected: false,
        port: Some(port.to_string()),
        chip: None,
        flash_size: None,
        flash_vendor: None,
        mac: None,
        board,
        unavailable_reason: Some("Could not read chip info over esptool — board may not be connected or in bootloader mode.".into()),
    }
}

fn run_generic_adapter(port: Option<&str>, board: Option<BoardBrief>) -> Telemetry {
    Telemetry {
        adapter: "generic".into(),
        connected: port.is_some(),
        port: port.map(str::to_string),
        chip: None,
        flash_size: None,
        flash_vendor: None,
        mac: None,
        board,
        unavailable_reason: if port.is_some() {
            None
        } else {
            Some("No device selected.".into())
        },
    }
}

/// `board` should come from the workspace's `board_id` looked up in the board catalogue
/// (`core::pio::boards::find_board`) — `None` when it can't be resolved, in which case this
/// still returns a usable (if sparser) `Telemetry` rather than erroring.
pub async fn get_telemetry(
    supervisor: &ProcessSupervisor,
    pio: Option<&Resolution>,
    cwd: &Path,
    cache: &EsptoolEntrypointCache,
    port: Option<&str>,
    board: Option<BoardBrief>,
) -> Telemetry {
    let is_esp = board.as_ref().map(|b| is_esp_platform(&b.platform)).unwrap_or(false);
    match (is_esp, pio) {
        (true, Some(pio)) if port.is_some() => run_esp_adapter(supervisor, pio, cwd, cache, port.unwrap(), board).await,
        (true, _) => Telemetry {
            adapter: "esp".into(),
            connected: false,
            port: port.map(str::to_string),
            chip: None,
            flash_size: None,
            flash_vendor: None,
            mac: None,
            board,
            unavailable_reason: Some("No device selected.".into()),
        },
        (false, _) => run_generic_adapter(port, board),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic shorter transcript (4MB flash, vs. the real board's 8MB) — kept as a
    /// second, independent shape check alongside the real captured fixture below.
    const SAMPLE_FLASH_ID_OUTPUT: &str = "\
Using tool-esptoolpy@1.40501.0 package
esptool.py v5.3.0
Serial port COM10:
Connecting.....
Connected to ESP32-C6 on COM10:
Chip type:          ESP32-C6 (QFN40) (revision v0.2)
Features:           Wi-Fi 6, BT 5 (LE), IEEE802.15.4, Single Core + LP Core, 160MHz, Unknown Embedded Flash
Crystal frequency:  40MHz
MAC:                10:51:db:ff:fe:03:97:dc

Manufacturer: c8
Device: 4016
Detected flash size: 4MB
Hard resetting via RTS pin...
";

    #[test]
    fn parses_chip_and_mac_from_the_real_connect_banner_shape() {
        let parsed = parse_flash_id_output(SAMPLE_FLASH_ID_OUTPUT);
        assert_eq!(parsed.chip.as_deref(), Some("ESP32-C6 (QFN40) (revision v0.2)"));
        assert_eq!(parsed.mac.as_deref(), Some("10:51:db:ff:fe:03:97:dc"));
        assert_eq!(parsed.flash_size.as_deref(), Some("4MB"));
        assert_eq!(parsed.flash_vendor.as_deref(), Some("c8"));
    }

    /// A real `pio pkg exec -p "tool-esptoolpy" -- esptool.py --port COM10 flash_id`
    /// capture against a physical ESP32-C6-DevKitM-1, including the two deprecation
    /// warning lines esptool v5.3.0 prints (`'esptool.py' is deprecated`, `'flash_id' is
    /// deprecated`) — confirms the parser tolerates extra noise between the package-prefix
    /// line and the connect banner.
    #[test]
    fn flash_id_args_matches_cli_contract_shape_with_an_explicit_port() {
        assert_eq!(
            flash_id_args("esptool.py", "COM10"),
            vec!["pkg", "exec", "-p", "tool-esptoolpy", "--", "esptool.py", "--port", "COM10", "flash_id"]
        );
        assert_eq!(
            flash_id_args("esptool", "/dev/ttyUSB0"),
            vec!["pkg", "exec", "-p", "tool-esptoolpy", "--", "esptool", "--port", "/dev/ttyUSB0", "flash_id"]
        );
    }

    #[test]
    fn real_captured_flash_id_output_parses_every_field() {
        let text = include_str!("../../../../tests/fixtures/esptool-flash-id-real.txt");
        let parsed = parse_flash_id_output(text);
        assert_eq!(parsed.chip.as_deref(), Some("ESP32-C6 (QFN40) (revision v0.2)"));
        assert_eq!(parsed.mac.as_deref(), Some("10:51:db:ff:fe:03:97:dc"));
        assert_eq!(parsed.flash_size.as_deref(), Some("8MB"));
        assert_eq!(parsed.flash_vendor.as_deref(), Some("c8"));
    }

    #[test]
    fn strips_the_using_package_prefix_line() {
        let stripped = strip_package_prefix(SAMPLE_FLASH_ID_OUTPUT);
        assert!(!stripped.starts_with("Using"));
        assert!(stripped.starts_with("esptool.py v5.3.0"));
    }

    #[test]
    fn unparseable_output_yields_all_none_fields_not_a_panic() {
        let parsed = parse_flash_id_output("garbage that matches nothing");
        assert!(parsed.chip.is_none());
        assert!(parsed.mac.is_none());
        assert!(parsed.flash_size.is_none());
        assert!(parsed.flash_vendor.is_none());
    }

    #[test]
    fn is_esp_platform_matches_espressif_families_case_insensitively() {
        assert!(is_esp_platform("espressif32"));
        assert!(is_esp_platform("espressif8266"));
        assert!(is_esp_platform("Espressif32"));
        assert!(!is_esp_platform("atmelavr"));
        assert!(!is_esp_platform("ststm32"));
    }

    #[test]
    fn generic_adapter_reports_unavailable_without_a_port() {
        let t = run_generic_adapter(None, None);
        assert_eq!(t.adapter, "generic");
        assert!(!t.connected);
        assert!(t.unavailable_reason.is_some());
    }

    #[test]
    fn generic_adapter_reports_connected_with_a_port_and_no_error() {
        let t = run_generic_adapter(Some("COM10"), None);
        assert_eq!(t.adapter, "generic");
        assert!(t.connected);
        assert!(t.unavailable_reason.is_none());
    }
}
