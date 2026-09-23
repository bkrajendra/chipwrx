//! Parses `pio project config --json-output` (`CLI-CONTRACT.md` §4.2) — the **effective**,
//! inheritance-resolved configuration. This is a read-only source for the "Effective"
//! column (`FR-INI-3`); it never drives writing, and it doesn't distinguish a section's own
//! declared options from ones it inherited — [`super::document`] does that by
//! cross-referencing against the declared model.
//!
//! Grounded in a real capture against a physical ESP32-C6-DevKitM-1 project with an
//! `extends` chain and a global `[env]` section
//! (`tests/fixtures/pio-project-config-json-multienv-real.txt`).

use crate::error::{AppError, Result};
use std::collections::BTreeMap;

/// `sectionName -> optionName -> values`. Every value is stringified uniformly — a bare
/// string, a number, or an array — since `IniEntry.values: Vec<String>` (`IPC-CONTRACT.md`
/// §7) doesn't distinguish the underlying JSON type either.
pub type EffectiveConfig = BTreeMap<String, BTreeMap<String, Vec<String>>>;

fn stringify_value(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Array(items) => items.iter().map(stringify_scalar).collect(),
        other => vec![stringify_scalar(other)],
    }
}

fn stringify_scalar(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Parses the nested-array shape: `[[sectionName, [[optionName, value], ...]], ...]`.
pub fn parse_effective_config(json: &str) -> Result<EffectiveConfig> {
    let raw: serde_json::Value = serde_json::from_str(json).map_err(|e| AppError::Io {
        message: format!("parsing `pio project config --json-output`: {e}"),
    })?;
    let sections = raw.as_array().ok_or_else(|| AppError::Io {
        message: "`pio project config --json-output` was not a JSON array".into(),
    })?;

    let mut out = EffectiveConfig::new();
    for section_entry in sections {
        let Some(pair) = section_entry.as_array() else { continue };
        let (Some(name), Some(options)) = (pair.first().and_then(|v| v.as_str()), pair.get(1).and_then(|v| v.as_array())) else {
            continue;
        };
        let mut option_map = BTreeMap::new();
        for option_entry in options {
            let Some(opair) = option_entry.as_array() else { continue };
            let (Some(oname), Some(ovalue)) = (opair.first().and_then(|v| v.as_str()), opair.get(1)) else {
                continue;
            };
            option_map.insert(oname.to_string(), stringify_value(ovalue));
        }
        out.insert(name.to_string(), option_map);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_real_multienv_capture_with_inheritance_and_extends() {
        let json = include_str!("../../../../tests/fixtures/pio-project-config-json-multienv-real.txt");
        let cfg = parse_effective_config(json).expect("parse");

        assert_eq!(cfg["platformio"]["default_envs"], vec!["esp32-c6-devkitm-1"]);
        assert_eq!(cfg["env"]["monitor_speed"], vec!["115200"]);

        let esp = &cfg["env:esp32-c6-devkitm-1"];
        assert_eq!(esp["board"], vec!["esp32-c6-devkitm-1"]);
        assert_eq!(esp["build_flags"], vec!["-DDEBUG=1", "-Wall"]);
        // inherited from [env], not declared in this section
        assert_eq!(esp["monitor_speed"], vec!["115200"]);

        let ota = &cfg["env:esp32-c6-devkitm-1-ota"];
        assert_eq!(ota["upload_protocol"], vec!["espota"]);
        // pulled in via `extends`
        assert_eq!(ota["board"], vec!["esp32-c6-devkitm-1"]);
        assert_eq!(ota["monitor_speed"], vec!["115200"]);
    }

    #[test]
    fn parses_a_section_with_no_env_or_platformio_prefix() {
        let json = include_str!("../../../../tests/fixtures/pio-project-config-json-real.txt");
        let cfg = parse_effective_config(json).expect("parse");
        assert_eq!(cfg["env:esp32-c6-devkitm-1"]["lib_deps"], vec!["knolleary/PubSubClient@^2.8", "bblanchon/ArduinoJson@^7.0.0"]);
    }

    #[test]
    fn malformed_json_returns_a_typed_error_not_a_panic() {
        assert!(parse_effective_config("not json").is_err());
        assert!(parse_effective_config("{}").is_err()); // valid JSON, but not the documented array shape
    }

    #[test]
    fn an_empty_array_yields_an_empty_config() {
        let cfg = parse_effective_config("[]").expect("parse");
        assert!(cfg.is_empty());
    }
}
