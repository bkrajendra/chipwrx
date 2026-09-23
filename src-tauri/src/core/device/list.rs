//! `pio device list --serial --json-output` (`CLI-CONTRACT.md` §3.2). Minimal and
//! stateless — no leasing, no hot-plug tracking, no re-enumeration retry. `SPEC.md` §8
//! open question 17: those are `PortBroker`'s job (M6); this exists only so M5's Upload
//! flow has *something* to pick a port from.

use crate::core::proc::{ProcKind, ProcessSupervisor, SpawnSpec};
use crate::core::toolchain::resolve::Resolution;
use crate::error::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::LazyLock;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SerialDevice {
    pub port: String,
    pub description: String,
    pub hwid: String,
    pub vid: Option<String>,
    pub pid: Option<String>,
    pub serial: Option<String>,
    /// `"CH340" | "CP210x" | "FTDI"` from a well-known USB-serial bridge VID; `None` for
    /// anything else (including Espressif's own native-USB VIDs — not confidently mapped
    /// to a single label, so left unset rather than guessed).
    pub known_bridge: Option<String>,
}

#[derive(Deserialize)]
struct RawDevice {
    port: String,
    description: String,
    hwid: String,
}

static VID_PID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"VID:PID=([0-9A-Fa-f]{4}):([0-9A-Fa-f]{4})").unwrap());
static SER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"SER=(\S+)").unwrap());

fn known_bridge_for(vid: &str) -> Option<String> {
    match vid.to_uppercase().as_str() {
        "10C4" => Some("CP210x".to_string()),
        "1A86" => Some("CH340".to_string()),
        "0403" => Some("FTDI".to_string()),
        _ => None,
    }
}

fn parse_device_list(json: &str) -> Vec<SerialDevice> {
    let Ok(raw): std::result::Result<Vec<RawDevice>, _> = serde_json::from_str(json) else {
        return Vec::new();
    };
    raw.into_iter()
        .map(|d| {
            let vid = VID_PID_RE.captures(&d.hwid).map(|c| c[1].to_uppercase());
            let pid = VID_PID_RE.captures(&d.hwid).map(|c| c[2].to_uppercase());
            let serial = SER_RE.captures(&d.hwid).map(|c| c[1].to_string());
            let known_bridge = vid.as_deref().and_then(known_bridge_for);
            SerialDevice {
                port: d.port,
                description: d.description,
                hwid: d.hwid,
                vid,
                pid,
                serial,
                known_bridge,
            }
        })
        .collect()
}

/// `pio device list --serial --json-output` — always `--serial` alone, per
/// `CLI-CONTRACT.md` §3.2: "Output shape depends on how many kinds were requested... Always
/// request exactly one kind so the shape is stable."
pub async fn list_serial_devices(supervisor: &ProcessSupervisor, pio: &Resolution, cwd: &Path) -> Result<Vec<SerialDevice>> {
    let mut args = pio.extra_args.clone();
    args.extend(["device".into(), "list".into(), "--serial".into(), "--json-output".into()]);
    let output = supervisor
        .spawn_capture(SpawnSpec {
            program: pio.program.clone(),
            args,
            cwd: cwd.to_path_buf(),
            env: vec![],
            kind: ProcKind::Pio,
            label: "pio-device-list".into(),
        })
        .await?;
    Ok(parse_device_list(&output.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_real_captured_fixture() {
        let json = include_str!("../../../../tests/fixtures/pio-device-list.json");
        let devices = parse_device_list(json);
        assert_eq!(devices.len(), 5);

        let cp210x = devices.iter().find(|d| d.port == "COM10").expect("COM10");
        assert_eq!(cp210x.vid.as_deref(), Some("10C4"));
        assert_eq!(cp210x.pid.as_deref(), Some("EA60"));
        assert_eq!(cp210x.serial.as_deref(), Some("28458697B9A1EF119EEC8E6661CE3355"));
        assert_eq!(cp210x.known_bridge.as_deref(), Some("CP210x"));

        let bluetooth = devices.iter().find(|d| d.port == "COM6").expect("COM6");
        assert_eq!(bluetooth.vid, None);
        assert_eq!(bluetooth.known_bridge, None);
    }

    #[test]
    fn unparseable_output_yields_an_empty_list_not_a_panic() {
        assert_eq!(parse_device_list("not json").len(), 0);
        assert_eq!(parse_device_list("").len(), 0);
    }

    #[test]
    fn recognizes_ch340_and_ftdi_vids() {
        let json = r#"[
            {"port":"/dev/ttyUSB0","description":"CH340","hwid":"USB VID:PID=1A86:7523 LOCATION=1-1"},
            {"port":"/dev/ttyUSB1","description":"FTDI","hwid":"USB VID:PID=0403:6001 LOCATION=1-2"}
        ]"#;
        let devices = parse_device_list(json);
        assert_eq!(devices[0].known_bridge.as_deref(), Some("CH340"));
        assert_eq!(devices[1].known_bridge.as_deref(), Some("FTDI"));
    }
}
