//! Parses `pio pkg list` / `pio pkg outdated` (`FR-INI-9`, `CLI-CONTRACT.md` §7.3) — neither
//! supports `--json-output` in Core 6.2.0, so both are tree/table text formats. Also builds
//! the `pio pkg install`/`uninstall` argv (`FR-INI-8`, `CLI-CONTRACT.md` §7.2).
//!
//! Grounded in real captures against a physical project this session:
//! `tests/fixtures/pio-pkg-list-real.txt` (one platform with its nested tool/framework
//! packages, then a `Libraries` section) and `pio-pkg-outdated-real.txt` (one library
//! pinned to an old version to force a real outdated row).

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::LazyLock;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PkgKind {
    Platform,
    Tool,
    Library,
}

impl PkgKind {
    /// The `pio pkg install`/`uninstall` flag that selects this kind — `-p`, `-t`, `-l`.
    pub fn flag(self) -> &'static str {
        match self {
            PkgKind::Platform => "-p",
            PkgKind::Tool => "-t",
            PkgKind::Library => "-l",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    pub required_spec: Option<String>,
    pub kind: PkgKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OutdatedPackage {
    pub name: String,
    pub current: String,
    pub wanted: String,
    pub latest: String,
    pub kind: String,
    pub environments: Vec<String>,
}

static ROW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:[├└]──\s+)?(?P<name>.+?)\s+@\s+(?P<version>\S+)(?:\s+\(required:\s*(?P<spec>.+)\))?$").unwrap());

/// Parses `pio pkg list`'s tree output. Tolerant: a line that doesn't match the expected
/// `name @ version` shape (a stray blank line, a future format tweak) is simply skipped —
/// treated as informational, never a parse failure (`CLI-CONTRACT.md` §7.3).
pub fn parse_pkg_list(text: &str) -> Vec<InstalledPackage> {
    let mut out = Vec::new();
    let mut nested_kind = PkgKind::Tool;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("Resolving ") {
            continue;
        }
        if trimmed == "Libraries" {
            nested_kind = PkgKind::Library;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Platform ") {
            if let Some(caps) = ROW.captures(rest) {
                out.push(InstalledPackage {
                    name: caps["name"].trim().to_string(),
                    version: caps["version"].to_string(),
                    required_spec: caps.name("spec").map(|m| m.as_str().to_string()),
                    kind: PkgKind::Platform,
                });
            }
            continue;
        }
        if let Some(caps) = ROW.captures(trimmed) {
            out.push(InstalledPackage {
                name: caps["name"].trim().to_string(),
                version: caps["version"].to_string(),
                required_spec: caps.name("spec").map(|m| m.as_str().to_string()),
                kind: nested_kind,
            });
        }
    }
    out
}

/// Splits on runs of 2+ spaces, matching the column padding `pio pkg outdated`/
/// `pio settings get` use — the same "no safe single delimiter, but columns are always
/// separated by at least two spaces" shape `core::ini::lint` already relies on.
fn split_columns(line: &str) -> Vec<&str> {
    static SEP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}").unwrap());
    SEP.split(line.trim()).collect()
}

/// Parses `pio pkg outdated`'s table. `"Everything is up-to-date!"` (no header row at all)
/// yields an empty list, not an error.
pub fn parse_pkg_outdated(text: &str) -> Vec<OutdatedPackage> {
    let lines: Vec<&str> = text.lines().collect();
    let Some(header_idx) = lines.iter().position(|l| l.trim_start().starts_with("Package")) else {
        return Vec::new();
    };
    // header_idx + 1 is the dashed separator row — data starts after that.
    lines
        .iter()
        .skip(header_idx + 2)
        .filter_map(|line| {
            if line.trim().is_empty() {
                return None;
            }
            let cols = split_columns(line);
            if cols.len() < 6 {
                return None;
            }
            Some(OutdatedPackage {
                name: cols[0].to_string(),
                current: cols[1].to_string(),
                wanted: cols[2].to_string(),
                latest: cols[3].to_string(),
                kind: cols[4].to_string(),
                environments: cols[5].split_whitespace().map(String::from).collect(),
            })
        })
        .collect()
}

/// `pio pkg install -d <dir> -e <env> <kind.flag()> "<spec>"` (`CLI-CONTRACT.md` §7.2).
pub fn install_args(dir: &Path, env: &str, kind: PkgKind, spec: &str) -> Vec<String> {
    vec![
        "pkg".into(),
        "install".into(),
        "-d".into(),
        dir.display().to_string(),
        "-e".into(),
        env.into(),
        kind.flag().into(),
        spec.into(),
    ]
}

/// `pio pkg uninstall` takes the same shape as `install` (`CLI-CONTRACT.md` §7.2).
pub fn uninstall_args(dir: &Path, env: &str, kind: PkgKind, spec: &str) -> Vec<String> {
    let mut args = install_args(dir, env, kind, spec);
    args[1] = "uninstall".into();
    args
}

pub fn list_args(dir: &Path, env: &str) -> Vec<String> {
    vec!["pkg".into(), "list".into(), "-d".into(), dir.display().to_string(), "-e".into(), env.into()]
}

pub fn outdated_args(dir: &Path, env: &str) -> Vec<String> {
    vec!["pkg".into(), "outdated".into(), "-d".into(), dir.display().to_string(), "-e".into(), env.into()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_the_real_captured_pkg_list() {
        let text = include_str!("../../../../tests/fixtures/pio-pkg-list-real.txt");
        let packages = parse_pkg_list(text);

        let platform = packages.iter().find(|p| p.kind == PkgKind::Platform).expect("a platform row");
        assert_eq!(platform.name, "espressif32");
        assert_eq!(platform.version, "55.3.311");
        assert!(platform.required_spec.as_deref().unwrap().starts_with("https://github.com/pioarduino"));

        let tools: Vec<_> = packages.iter().filter(|p| p.kind == PkgKind::Tool).collect();
        assert_eq!(tools.len(), 6);
        assert!(tools.iter().any(|t| t.name == "tool-esptoolpy" && t.version == "5.3.0"));

        let libs: Vec<_> = packages.iter().filter(|p| p.kind == PkgKind::Library).collect();
        assert_eq!(libs.len(), 2);
        let arduino_json = libs.iter().find(|l| l.name == "ArduinoJson").expect("ArduinoJson");
        assert_eq!(arduino_json.required_spec.as_deref(), Some("bblanchon/ArduinoJson @ 6.21.5"));
    }

    #[test]
    fn unparseable_pkg_list_output_yields_an_empty_list_not_a_panic() {
        assert!(parse_pkg_list("garbage\nmore garbage").is_empty());
        assert!(parse_pkg_list("").is_empty());
    }

    #[test]
    fn parses_the_real_captured_outdated_table() {
        let text = include_str!("../../../../tests/fixtures/pio-pkg-outdated-real.txt");
        let outdated = parse_pkg_outdated(text);
        assert_eq!(outdated.len(), 1);
        let row = &outdated[0];
        assert_eq!(row.name, "ArduinoJson");
        assert_eq!(row.current, "6.21.5");
        assert_eq!(row.wanted, "6.21.5");
        assert_eq!(row.latest, "7.4.3");
        assert_eq!(row.kind, "Library");
        assert_eq!(row.environments, vec!["esp32-c6-devkitm-1"]);
    }

    #[test]
    fn everything_up_to_date_yields_an_empty_list() {
        let text = "Checking\nEverything is up-to-date!\n";
        assert!(parse_pkg_outdated(text).is_empty());
    }

    #[test]
    fn install_args_matches_cli_contract_shape() {
        assert_eq!(
            install_args(&PathBuf::from("/ws"), "esp32dev", PkgKind::Library, "bblanchon/ArduinoJson@^7.0.0"),
            vec!["pkg", "install", "-d", "/ws", "-e", "esp32dev", "-l", "bblanchon/ArduinoJson@^7.0.0"]
        );
        assert_eq!(
            install_args(&PathBuf::from("/ws"), "esp32dev", PkgKind::Platform, "espressif32"),
            vec!["pkg", "install", "-d", "/ws", "-e", "esp32dev", "-p", "espressif32"]
        );
    }

    #[test]
    fn uninstall_args_matches_install_shape_with_uninstall_swapped_in() {
        assert_eq!(
            uninstall_args(&PathBuf::from("/ws"), "esp32dev", PkgKind::Library, "bblanchon/ArduinoJson"),
            vec!["pkg", "uninstall", "-d", "/ws", "-e", "esp32dev", "-l", "bblanchon/ArduinoJson"]
        );
    }

    #[test]
    fn list_and_outdated_args_match_cli_contract_shape() {
        assert_eq!(list_args(&PathBuf::from("/ws"), "esp32dev"), vec!["pkg", "list", "-d", "/ws", "-e", "esp32dev"]);
        assert_eq!(outdated_args(&PathBuf::from("/ws"), "esp32dev"), vec!["pkg", "outdated", "-d", "/ws", "-e", "esp32dev"]);
    }
}
