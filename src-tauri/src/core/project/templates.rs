//! Saved `platformio.ini` templates (`FR-INI-11`, `DATA-MODEL.md` §10) —
//! `<config>/templates/<slug>.json`, one file per template.
//!
//! "Applying a template merges `[env:*]` sections into the current file rather than
//! replacing it" (`DATA-MODEL.md` §10) — only `env:*` sections are merged; a template never
//! touches the target project's own `[platformio]`/`[env]` global sections. Applying
//! writes immediately and returns the resulting document (`IPC-CONTRACT.md` §7's
//! `ini_template_apply -> IniDocument`, matching `ini_apply`/`ini_write_raw`'s own
//! always-returns-post-write-state shape) — there's no separate non-mutating preview
//! command in the given IPC surface, so "always shows the resulting diff for confirmation
//! before writing" is approximated by the frontend diffing the already-cached pre-apply
//! `IniDocument.raw` against this call's returned one, with straightforward undo via
//! `ini_write_raw` (`SPEC.md` §8 open question 33).

use super::super::ini::parse::parse;
use super::super::ini::patch::IniEdit;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct IniTemplate {
    pub schema_version: u32,
    pub name: String,
    /// RFC3339.
    pub created_at: String,
    pub ini: String,
    pub board_id: Option<String>,
    pub claude_md: Option<String>,
}

const CURRENT_SCHEMA_VERSION: u32 = 1;

fn templates_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("templates")
}

/// Lowercase, non-alphanumeric runs collapsed to a single `-`, trimmed — a filesystem- and
/// URL-safe stand-in for the template's display name.
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut last_was_dash = false;
    for ch in name.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_was_dash = false;
        } else if !last_was_dash && !out.is_empty() {
            out.push('-');
            last_was_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "template".to_string()
    } else {
        out
    }
}

fn template_path(config_dir: &Path, slug: &str) -> PathBuf {
    templates_dir(config_dir).join(format!("{slug}.json"))
}

/// Builds a new `IniTemplate` from the current moment and saves it — `created_at` is
/// always `now`, matching "saving the current `platformio.ini` as a named template" being
/// a snapshot action, not an editable record.
pub fn save(config_dir: &Path, name: &str, ini: String, board_id: Option<String>, claude_md: Option<String>) -> Result<IniTemplate> {
    let template = IniTemplate {
        schema_version: CURRENT_SCHEMA_VERSION,
        name: name.to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        ini,
        board_id,
        claude_md,
    };
    let dir = templates_dir(config_dir);
    std::fs::create_dir_all(&dir)?;
    let slug = slugify(&template.name);
    let path = template_path(config_dir, &slug);
    let tmp = dir.join(format!("{slug}.json.tmp-{}", std::process::id()));
    let json = serde_json::to_vec_pretty(&template)?;
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&json)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(template)
}

/// Every template on disk, skipping any file that fails to parse (a hand-edited or
/// partially-written file degrades to "not shown," never a crash for the whole list).
pub fn list(config_dir: &Path) -> Vec<IniTemplate> {
    let Ok(entries) = std::fs::read_dir(templates_dir(config_dir)) else {
        return Vec::new();
    };
    let mut templates: Vec<IniTemplate> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter_map(|bytes| serde_json::from_slice(&bytes).ok())
        .collect();
    templates.sort_by(|a: &IniTemplate, b: &IniTemplate| a.name.cmp(&b.name));
    templates
}

pub fn load(config_dir: &Path, slug: &str) -> Option<IniTemplate> {
    let bytes = std::fs::read(template_path(config_dir, slug)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn find_by_name(config_dir: &Path, name: &str) -> Option<IniTemplate> {
    load(config_dir, &slugify(name))
}

/// Builds the `IniEdit`s that merge this template's `env:*` sections into a target project.
/// Each such section becomes an idempotent `AddSection` (a no-op if it already exists)
/// followed by one `Set` per declared option in that section — overwriting the target's
/// existing value for that specific key if present, leaving every other key and every
/// non-`env:*` section (comments, `[platformio]`, the global `[env]`) completely untouched.
pub fn merge_edits(template: &IniTemplate) -> Vec<IniEdit> {
    let doc = parse(&template.ini);
    let mut edits = Vec::new();
    for section in &doc.sections {
        if !section.name.starts_with("env:") {
            continue;
        }
        edits.push(IniEdit::AddSection { name: section.name.clone() });
        for entry in &section.entries {
            edits.push(IniEdit::Set {
                section: section.name.clone(),
                name: entry.name.clone(),
                values: entry.values.clone(),
            });
        }
    }
    edits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ini::patch::apply_edits;

    fn tempdir() -> PathBuf {
        std::env::temp_dir().join(format!("vibe-hw-templates-test-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn slugify_lowercases_and_collapses_punctuation() {
        assert_eq!(slugify("ESP32 + MQTT starter"), "esp32-mqtt-starter");
        assert_eq!(slugify("  leading/trailing  "), "leading-trailing");
        assert_eq!(slugify("---"), "template");
        assert_eq!(slugify(""), "template");
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempdir();
        let saved = save(&dir, "ESP32 starter", "[env:esp32dev]\nboard = esp32dev\n".into(), Some("esp32dev".into()), None).expect("save");
        let loaded = load(&dir, &slugify("ESP32 starter")).expect("load");
        assert_eq!(loaded, saved);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_returns_every_saved_template_sorted_by_name() {
        let dir = tempdir();
        save(&dir, "Zebra template", "[env:z]\n".into(), None, None).expect("save z");
        save(&dir, "Alpha template", "[env:a]\n".into(), None, None).expect("save a");
        let templates = list(&dir);
        assert_eq!(templates.len(), 2);
        assert_eq!(templates[0].name, "Alpha template");
        assert_eq!(templates[1].name, "Zebra template");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_templates_dir_yields_an_empty_list_not_an_error() {
        let dir = tempdir();
        assert!(list(&dir).is_empty());
    }

    #[test]
    fn find_by_name_resolves_through_the_same_slug() {
        let dir = tempdir();
        save(&dir, "My Board Setup", "[env:x]\n".into(), None, None).expect("save");
        assert!(find_by_name(&dir, "My Board Setup").is_some());
        assert!(find_by_name(&dir, "does not exist").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_edits_only_touches_env_sections() {
        let template = IniTemplate {
            schema_version: 1,
            name: "t".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            ini: "[platformio]\ndefault_envs = esp32dev\n\n[env:esp32dev]\nboard = esp32dev\nmonitor_speed = 115200\n".into(),
            board_id: None,
            claude_md: None,
        };
        let edits = merge_edits(&template);
        // [platformio] is never in the edit list
        assert!(!edits.iter().any(|e| matches!(e, IniEdit::AddSection { name } if name == "platformio")));
        assert!(edits.iter().any(|e| matches!(e, IniEdit::AddSection { name } if name == "env:esp32dev")));
        assert!(edits.iter().any(|e| matches!(e, IniEdit::Set { name, .. } if name == "board")));
        assert!(edits.iter().any(|e| matches!(e, IniEdit::Set { name, .. } if name == "monitor_speed")));
    }

    #[test]
    fn merging_into_an_existing_project_overwrites_only_the_named_keys() {
        let target = "[platformio]\ndefault_envs = esp32dev\n\n[env:esp32dev]\nboard = esp32dev\nframework = arduino\n";
        let template = IniTemplate {
            schema_version: 1,
            name: "t".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            ini: "[env:esp32dev]\nmonitor_speed = 115200\n".into(),
            board_id: None,
            claude_md: None,
        };
        let merged = apply_edits(target, &merge_edits(&template));
        assert!(merged.contains("framework = arduino")); // untouched
        assert!(merged.contains("monitor_speed = 115200")); // added by the template
        assert!(merged.contains("default_envs = esp32dev")); // [platformio] untouched
    }
}
