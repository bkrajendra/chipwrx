//! Parses `pio settings get` (`FR-INI-6`, `CLI-CONTRACT.md` §2.3) — no `--json-output`, a
//! padded text table. Setting *names* are read from whatever the table actually contains
//! rather than a hardcoded list: a real capture against this session's installed Core only
//! showed 6 of the 8 names `CLI-CONTRACT.md` documents (`enable_telemetry` and
//! `disable_udev_rules_check` returned empty tables when queried individually — apparently
//! not present in this Core version) — `SPEC.md` §8 open question 32.

use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PioSetting {
    pub name: String,
    pub current_value: String,
    /// Only known when the row's value column showed a `[default]` suffix — a real capture
    /// against every-setting-at-its-default never showed one, so whether/how a
    /// *non-default* row's bracket looks is unverified (`SPEC.md` §8 open question 32).
    pub default_value: Option<String>,
    pub description: String,
}

static COLUMN_SEP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}").unwrap());
static BRACKETED_DEFAULT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?P<current>.*\S)\s*\[(?P<default>.*)\]$").unwrap());

fn split_columns(line: &str, n: usize) -> Option<Vec<String>> {
    let mut parts = Vec::with_capacity(n);
    let mut rest = line.trim();
    for _ in 0..n - 1 {
        let m = COLUMN_SEP.find(rest)?;
        parts.push(rest[..m.start()].to_string());
        rest = &rest[m.end()..];
    }
    parts.push(rest.to_string());
    Some(parts)
}

fn split_value_and_default(column: &str) -> (String, Option<String>) {
    match BRACKETED_DEFAULT.captures(column) {
        Some(caps) => (caps["current"].to_string(), Some(caps["default"].to_string())),
        None => (column.trim().to_string(), None),
    }
}

/// Parses `pio settings get`'s table. Tolerant of extra/unknown rows (a future PlatformIO
/// version adding a setting shows up automatically, no code change needed) and of a
/// missing header (returns an empty list rather than erroring).
pub fn parse_settings_get(text: &str) -> Vec<PioSetting> {
    let lines: Vec<&str> = text.lines().collect();
    let Some(header_idx) = lines.iter().position(|l| l.trim_start().starts_with("Name")) else {
        return Vec::new();
    };
    lines
        .iter()
        .skip(header_idx + 2) // header + dashed separator row
        .filter_map(|line| {
            if line.trim().is_empty() {
                return None;
            }
            let cols = split_columns(line, 3)?;
            let (current_value, default_value) = split_value_and_default(&cols[1]);
            Some(PioSetting {
                name: cols[0].trim().to_string(),
                current_value,
                default_value,
                description: cols[2].trim().to_string(),
            })
        })
        .collect()
}

pub fn get_args() -> Vec<String> {
    vec!["settings".into(), "get".into()]
}

pub fn set_args(name: &str, value: &str) -> Vec<String> {
    vec!["settings".into(), "set".into(), name.into(), value.into()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_real_captured_settings_table() {
        let text = include_str!("../../../../tests/fixtures/pio-settings-get-real.txt");
        let settings = parse_settings_get(text);
        assert_eq!(settings.len(), 6);

        let interval = settings.iter().find(|s| s.name == "check_platformio_interval").expect("check_platformio_interval");
        assert_eq!(interval.current_value, "7");
        assert_eq!(interval.description, "Check for the new PlatformIO Core interval (days)");

        let projects_dir = settings.iter().find(|s| s.name == "projects_dir").expect("projects_dir");
        assert_eq!(projects_dir.current_value, "C:\\Users\\resea\\OneDrive\\Documents\\PlatformIO\\Projects");

        let enable_cache = settings.iter().find(|s| s.name == "enable_cache").expect("enable_cache");
        assert_eq!(enable_cache.current_value, "Yes");
    }

    #[test]
    fn splits_a_bracketed_default_when_present() {
        let (current, default) = split_value_and_default("115200 [9600]");
        assert_eq!(current, "115200");
        assert_eq!(default.as_deref(), Some("9600"));
    }

    #[test]
    fn a_value_with_no_bracket_has_no_default() {
        let (current, default) = split_value_and_default("Yes");
        assert_eq!(current, "Yes");
        assert_eq!(default, None);
    }

    #[test]
    fn no_header_returns_an_empty_list_not_an_error() {
        assert!(parse_settings_get("nothing here").is_empty());
        assert!(parse_settings_get("").is_empty());
    }

    #[test]
    fn get_and_set_args_match_cli_contract_shape() {
        assert_eq!(get_args(), vec!["settings", "get"]);
        assert_eq!(set_args("enable_cache", "No"), vec!["settings", "set", "enable_cache", "No"]);
    }
}
