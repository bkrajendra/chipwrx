//! Pure hot-plug diffing (`FR-DEV-8`) and sticky re-binding matching (`FR-DEV-2`). Kept
//! free of any I/O so it is unit-testable without a fake CLI or a real port.

use super::list::SerialDevice;
use crate::core::project::types::StickyHwid;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceDiff {
    pub added: Vec<SerialDevice>,
    pub removed: Vec<String>,
}

impl DeviceDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

/// The wire shape for the global `device://changed` event (`IPC-CONTRACT.md` §6: "carries
/// `{ added: SerialDevice[], removed: string[] }`").
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DeviceChanged {
    pub added: Vec<SerialDevice>,
    pub removed: Vec<String>,
}

impl From<DeviceDiff> for DeviceChanged {
    fn from(d: DeviceDiff) -> Self {
        Self {
            added: d.added,
            removed: d.removed,
        }
    }
}

/// Ports present in `current` but not `previous` are "added"; ports present in `previous`
/// but not `current` are "removed". Matched purely by port path — a device that reappears
/// on a *different* path shows up as one remove plus one add, which is exactly what
/// `FR-DEV-2`'s stickiness logic (see [`matches_sticky`]) needs to tell apart from a
/// brand-new device.
pub fn diff_devices(previous: &[SerialDevice], current: &[SerialDevice]) -> DeviceDiff {
    let added = current
        .iter()
        .filter(|d| !previous.iter().any(|p| p.port == d.port))
        .cloned()
        .collect();
    let removed = previous
        .iter()
        .filter(|p| !current.iter().any(|d| d.port == p.port))
        .map(|p| p.port.clone())
        .collect();
    DeviceDiff { added, removed }
}

/// A device matches a stored `StickyHwid` when its VID:PID agree, and — only if the sticky
/// record has a serial recorded — its serial also agrees. `vid`/`pid` are compared
/// case-insensitively since `core::device::list` always uppercases them but a persisted
/// `StickyHwid` could in principle have been hand-edited.
pub fn matches_sticky(device: &SerialDevice, sticky: &StickyHwid) -> bool {
    let vid_matches = device.vid.as_deref().map(|v| v.eq_ignore_ascii_case(&sticky.vid)).unwrap_or(false);
    let pid_matches = device.pid.as_deref().map(|p| p.eq_ignore_ascii_case(&sticky.pid)).unwrap_or(false);
    if !vid_matches || !pid_matches {
        return false;
    }
    match &sticky.serial {
        Some(want) => device.serial.as_deref().map(|s| s == want).unwrap_or(false),
        None => true,
    }
}

pub fn sticky_hwid_for(device: &SerialDevice) -> Option<StickyHwid> {
    Some(StickyHwid {
        vid: device.vid.clone()?,
        pid: device.pid.clone()?,
        serial: device.serial.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(port: &str, vid: Option<&str>, pid: Option<&str>, serial: Option<&str>) -> SerialDevice {
        SerialDevice {
            port: port.into(),
            description: "test device".into(),
            hwid: "n/a".into(),
            vid: vid.map(str::to_string),
            pid: pid.map(str::to_string),
            serial: serial.map(str::to_string),
            known_bridge: None,
        }
    }

    #[test]
    fn no_change_yields_an_empty_diff() {
        let a = vec![dev("COM3", Some("10C4"), Some("EA60"), Some("SER1"))];
        let diff = diff_devices(&a, &a.clone());
        assert!(diff.is_empty());
    }

    #[test]
    fn a_new_port_is_added() {
        let previous = vec![];
        let current = vec![dev("COM3", None, None, None)];
        let diff = diff_devices(&previous, &current);
        assert_eq!(diff.added.len(), 1);
        assert!(diff.removed.is_empty());
    }

    #[test]
    fn a_vanished_port_is_removed() {
        let previous = vec![dev("COM3", None, None, None)];
        let current = vec![];
        let diff = diff_devices(&previous, &current);
        assert!(diff.added.is_empty());
        assert_eq!(diff.removed, vec!["COM3".to_string()]);
    }

    #[test]
    fn a_device_reappearing_on_a_different_port_is_one_remove_and_one_add() {
        let previous = vec![dev("COM3", Some("10C4"), Some("EA60"), Some("SER1"))];
        let current = vec![dev("COM5", Some("10C4"), Some("EA60"), Some("SER1"))];
        let diff = diff_devices(&previous, &current);
        assert_eq!(diff.removed, vec!["COM3".to_string()]);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.added[0].port, "COM5");
    }

    #[test]
    fn matches_sticky_requires_vid_and_pid() {
        let sticky = StickyHwid {
            vid: "10C4".into(),
            pid: "EA60".into(),
            serial: None,
        };
        assert!(matches_sticky(&dev("COM5", Some("10C4"), Some("EA60"), Some("anything")), &sticky));
        assert!(!matches_sticky(&dev("COM5", Some("10C4"), Some("0000"), None), &sticky));
        assert!(!matches_sticky(&dev("COM5", None, None, None), &sticky));
    }

    #[test]
    fn matches_sticky_checks_serial_when_recorded() {
        let sticky = StickyHwid {
            vid: "10C4".into(),
            pid: "EA60".into(),
            serial: Some("SER1".into()),
        };
        assert!(matches_sticky(&dev("COM5", Some("10C4"), Some("EA60"), Some("SER1")), &sticky));
        assert!(!matches_sticky(&dev("COM5", Some("10C4"), Some("EA60"), Some("SER2")), &sticky));
        assert!(!matches_sticky(&dev("COM5", Some("10C4"), Some("EA60"), None), &sticky));
    }

    #[test]
    fn matches_sticky_is_case_insensitive_on_vid_pid() {
        let sticky = StickyHwid {
            vid: "10c4".into(),
            pid: "ea60".into(),
            serial: None,
        };
        assert!(matches_sticky(&dev("COM5", Some("10C4"), Some("EA60"), None), &sticky));
    }

    #[test]
    fn sticky_hwid_for_requires_vid_and_pid() {
        assert!(sticky_hwid_for(&dev("COM5", None, None, None)).is_none());
        let s = sticky_hwid_for(&dev("COM5", Some("10C4"), Some("EA60"), Some("SER1"))).expect("some");
        assert_eq!(s.vid, "10C4");
        assert_eq!(s.serial.as_deref(), Some("SER1"));
    }
}
