//! Reshapes the declared model ([`super::parse`]) and the effective model
//! ([`super::effective`]) into the IPC-facing [`IniDocument`] (`IPC-CONTRACT.md` §7),
//! computing each inherited option's `inherited_from` badge (`FR-INI-3`).

use super::effective::EffectiveConfig;
use super::parse::{self, IniDoc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct IniDocument {
    pub raw: String,
    pub sections: Vec<IniSection>,
    pub mtime_ms: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct IniSection {
    pub name: String,
    pub declared: Vec<IniEntry>,
    pub effective: Vec<IniEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct IniEntry {
    pub name: String,
    pub values: Vec<String>,
    /// `Some("env")` (or the target of an `extends` chain) when this option isn't declared
    /// in this section but the effective merge resolved a value for it anyway — `None`
    /// means it's either declared right here, or a PlatformIO built-in default with no
    /// traceable declaring section.
    pub inherited_from: Option<String>,
}

/// Only a declared `[env]` section, or an `extends` target, can be the traceable origin of
/// an inherited value — `pio project config --json-output`'s merged output doesn't itself
/// say which section a value came from, so this is reconstructed by checking the two
/// mechanisms PlatformIO actually supports for env-scoped option inheritance
/// (`CLI-CONTRACT.md` §4.2's own `[env]` example, and `extends`). A multi-level `extends`
/// chain resolves only one level deep — `SPEC.md` §8 open question 31.
fn resolve_inherited_from(doc: &IniDoc, section: &parse::Section, option_name: &str) -> Option<String> {
    if !section.name.starts_with("env:") {
        return None;
    }
    if let Some(env_section) = doc.section("env") {
        if env_section.entry(option_name).is_some() {
            return Some("env".to_string());
        }
    }
    if let Some(extends_entry) = section.entry("extends") {
        if let Some(target_name) = extends_entry.values.first() {
            if let Some(target_section) = doc.section(target_name) {
                if target_section.entry(option_name).is_some() {
                    return Some(target_name.clone());
                }
            }
        }
    }
    None
}

/// Builds the full IPC-facing document from a raw ini string, its already-fetched
/// effective config (`pio project config --json-output`, reshaped by
/// `super::effective::parse_effective_config`), and the file's on-disk mtime
/// (`ini_apply`/`ini_write_raw`'s changed-on-disk guard).
pub fn build_document(raw: &str, effective: &EffectiveConfig, mtime_ms: u64) -> IniDocument {
    let doc = parse::parse(raw);
    let mut sections = Vec::new();

    for section in &doc.sections {
        if section.name.is_empty() {
            continue; // the preamble pseudo-section isn't user-addressable
        }

        let declared: Vec<IniEntry> = section
            .entries
            .iter()
            .map(|e| IniEntry {
                name: e.name.clone(),
                values: e.values.clone(),
                inherited_from: None,
            })
            .collect();

        let mut effective_entries = Vec::new();
        if let Some(eff_map) = effective.get(&section.name) {
            for (option_name, values) in eff_map {
                if let Some(d) = declared.iter().find(|d| &d.name == option_name) {
                    effective_entries.push(IniEntry {
                        name: option_name.clone(),
                        values: d.values.clone(),
                        inherited_from: None,
                    });
                } else {
                    effective_entries.push(IniEntry {
                        name: option_name.clone(),
                        values: values.clone(),
                        inherited_from: resolve_inherited_from(&doc, section, option_name),
                    });
                }
            }
        }

        sections.push(IniSection {
            name: section.name.clone(),
            declared,
            effective: effective_entries,
        });
    }

    IniDocument {
        raw: raw.to_string(),
        sections,
        mtime_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ini::effective::parse_effective_config;

    #[test]
    fn monitor_speed_shows_as_inherited_from_env_in_the_named_env_section() {
        let raw = include_str!("../../../../tests/fixtures/platformio-real-multienv.ini");
        let json = include_str!("../../../../tests/fixtures/pio-project-config-json-multienv-real.txt");
        let effective = parse_effective_config(json).expect("parse effective");

        let document = build_document(raw, &effective, 0);
        let esp = document.sections.iter().find(|s| s.name == "env:esp32-c6-devkitm-1").expect("env section");

        assert!(!esp.declared.iter().any(|e| e.name == "monitor_speed"), "not declared directly in this section");
        let monitor_speed = esp.effective.iter().find(|e| e.name == "monitor_speed").expect("effective monitor_speed");
        assert_eq!(monitor_speed.values, vec!["115200"]);
        assert_eq!(monitor_speed.inherited_from.as_deref(), Some("env"));

        // board IS declared directly — no inherited badge
        let board = esp.effective.iter().find(|e| e.name == "board").expect("effective board");
        assert_eq!(board.inherited_from, None);
    }

    #[test]
    fn extends_chain_resolves_inherited_from_to_the_extended_section() {
        let raw = include_str!("../../../../tests/fixtures/platformio-real-multienv.ini");
        let json = include_str!("../../../../tests/fixtures/pio-project-config-json-multienv-real.txt");
        let effective = parse_effective_config(json).expect("parse effective");

        let document = build_document(raw, &effective, 0);
        let ota = document.sections.iter().find(|s| s.name == "env:esp32-c6-devkitm-1-ota").expect("ota section");

        let board = ota.effective.iter().find(|e| e.name == "board").expect("effective board");
        assert_eq!(board.inherited_from.as_deref(), Some("env:esp32-c6-devkitm-1"));

        // upload_protocol is declared directly in the ota section itself
        let upload_protocol = ota.effective.iter().find(|e| e.name == "upload_protocol").expect("effective upload_protocol");
        assert_eq!(upload_protocol.inherited_from, None);
    }

    #[test]
    fn the_preamble_pseudo_section_is_not_included_in_the_document() {
        let raw = "; a comment\n\n[env:x]\nboard = esp32dev\n";
        let effective = EffectiveConfig::new();
        let document = build_document(raw, &effective, 0);
        assert!(!document.sections.iter().any(|s| s.name.is_empty()));
    }

    #[test]
    fn raw_and_mtime_pass_through_unchanged() {
        let raw = "[env:x]\nboard = esp32dev\n";
        let document = build_document(raw, &EffectiveConfig::new(), 12345);
        assert_eq!(document.raw, raw);
        assert_eq!(document.mtime_ms, 12345);
    }
}
