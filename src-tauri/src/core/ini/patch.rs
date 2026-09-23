//! Surgical `platformio.ini` edits (`FR-INI-4`, `ARCHITECTURE.md` §3 `core/ini::patch`).
//! Each [`IniEdit`] re-parses the current text and rewrites only the line range it
//! actually touches — every other byte in the file is untouched. Applying an empty edit
//! list is a byte-identical no-op (see `parse`'s own round-trip tests); this module's
//! tests instead check that a *non-empty* edit leaves everything else byte-identical.

use super::parse::{detect_eol, parse, strip_eol, IniDoc};
use serde::Deserialize;
use ts_rs::TS;

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(tag = "type", content = "data", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum IniEdit {
    Set { section: String, name: String, values: Vec<String> },
    Remove { section: String, name: String },
    AddSection { name: String },
    RemoveSection { name: String },
    RenameSection { from: String, to: String },
}

/// The real PlatformIO writer's own shape for multi-value options, captured against a real
/// project this session: a single value stays on the `key = value` line; two or more switch
/// to `key = ` (**with a trailing space**) followed by one tab-indented continuation line
/// per value. Matching it exactly keeps our own writes indistinguishable from PlatformIO's,
/// which is what `FR-INI-4`'s "byte-identical" guarantee is actually protecting against —
/// noisy diffs from two competing ini-writing styles.
fn format_entry_lines(name: &str, values: &[String], eol: &str) -> Vec<String> {
    if values.len() <= 1 {
        let v = values.first().map(String::as_str).unwrap_or("");
        vec![format!("{name} = {v}{eol}")]
    } else {
        let mut out = vec![format!("{name} = {eol}")];
        out.extend(values.iter().map(|v| format!("\t{v}{eol}")));
        out
    }
}

/// Appends a brand-new, empty `[name]` section at end of file, adding exactly one blank
/// separator line first if the file has content that doesn't already end blank, and fixing
/// up a missing trailing newline on the previous last line first.
fn append_new_section(lines: &mut Vec<String>, name: &str, eol: &str) {
    if let Some(last) = lines.last_mut() {
        if !last.ends_with('\n') {
            last.push_str(eol);
        }
    }
    if let Some(last) = lines.last() {
        if !strip_eol(last).is_empty() {
            lines.push(eol.to_string());
        }
    }
    lines.push(format!("[{name}]{eol}"));
}

fn set_entry(doc: &IniDoc, section_name: &str, key: &str, values: &[String]) -> String {
    let eol = detect_eol(&doc.lines);

    let Some(section) = doc.section(section_name) else {
        let mut lines = doc.lines.clone();
        append_new_section(&mut lines, section_name, eol);
        // Re-parse: the section now exists, so a second pass sets the key inside it.
        return set_entry(&parse(&lines.concat()), section_name, key, values);
    };

    let mut lines = doc.lines.clone();
    let new_entry_lines = format_entry_lines(key, values, eol);
    match section.entry(key) {
        Some(entry) => {
            lines.splice(entry.start..entry.end, new_entry_lines);
        }
        None => {
            let insert_at = section.entries.last().map(|e| e.end).unwrap_or(section.start);
            // If inserting at the very end of the file and its last line has no trailing
            // newline (a file that doesn't end in one), splicing right after it would run
            // the new line straight onto the old one with no separator.
            if insert_at == lines.len() {
                if let Some(last) = lines.last_mut() {
                    if !last.ends_with('\n') {
                        last.push_str(eol);
                    }
                }
            }
            lines.splice(insert_at..insert_at, new_entry_lines);
        }
    }
    lines.concat()
}

fn remove_entry(doc: &IniDoc, section_name: &str, key: &str) -> String {
    let Some(section) = doc.section(section_name) else {
        return doc.raw();
    };
    let Some(entry) = section.entry(key) else {
        return doc.raw();
    };
    let mut lines = doc.lines.clone();
    lines.splice(entry.start..entry.end, std::iter::empty());
    lines.concat()
}

fn add_section(doc: &IniDoc, name: &str) -> String {
    if doc.section(name).is_some() {
        return doc.raw(); // idempotent — already exists
    }
    let eol = detect_eol(&doc.lines);
    let mut lines = doc.lines.clone();
    append_new_section(&mut lines, name, eol);
    lines.concat()
}

fn remove_section(doc: &IniDoc, name: &str) -> String {
    if name.is_empty() {
        return doc.raw(); // the preamble pseudo-section isn't removable
    }
    let Some(section) = doc.section(name) else {
        return doc.raw();
    };
    let Some(header) = section.header_line else {
        return doc.raw();
    };
    let mut lines = doc.lines.clone();
    lines.splice(header..section.end, std::iter::empty());
    lines.concat()
}

fn rename_section(doc: &IniDoc, from: &str, to: &str) -> String {
    let Some(section) = doc.section(from) else {
        return doc.raw();
    };
    let Some(header) = section.header_line else {
        return doc.raw();
    };
    let eol = detect_eol(&doc.lines);
    let mut lines = doc.lines.clone();
    lines[header] = format!("[{to}]{eol}");
    lines.concat()
}

pub fn apply_edit(raw: &str, edit: &IniEdit) -> String {
    let doc = parse(raw);
    match edit {
        IniEdit::Set { section, name, values } => set_entry(&doc, section, name, values),
        IniEdit::Remove { section, name } => remove_entry(&doc, section, name),
        IniEdit::AddSection { name } => add_section(&doc, name),
        IniEdit::RemoveSection { name } => remove_section(&doc, name),
        IniEdit::RenameSection { from, to } => rename_section(&doc, from, to),
    }
}

/// Applies `edits` in order, each against the result of the previous one. Re-parsing
/// between edits (rather than juggling line-index deltas across several splices) trades a
/// little performance — irrelevant for an ini file's size — for edits that are simple to
/// get right and easy to test in isolation.
pub fn apply_edits(raw: &str, edits: &[IniEdit]) -> String {
    let mut current = raw.to_string();
    for edit in edits {
        current = apply_edit(&current, edit);
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    const MULTI_ENV_INI: &str = "\
[platformio]
default_envs = esp32dev

[env]
monitor_speed = 115200

[env:esp32dev]
platform = espressif32
board = esp32dev
framework = arduino
build_flags =
\t-DDEBUG=1
\t-Wall

[env:uno]
platform = atmelavr
board = uno
";

    #[test]
    fn no_edits_is_a_byte_identical_no_op() {
        assert_eq!(apply_edits(MULTI_ENV_INI, &[]), MULTI_ENV_INI);
    }

    #[test]
    fn set_appends_a_new_value_to_an_existing_single_line_option_leaves_everything_else_identical() {
        let edited = apply_edit(
            MULTI_ENV_INI,
            &IniEdit::Set {
                section: "env:esp32dev".into(),
                name: "build_flags".into(),
                values: vec!["-DDEBUG=1".into(), "-Wall".into(), "-DFOO=1".into()],
            },
        );
        let expected = MULTI_ENV_INI.replace("build_flags =\n\t-DDEBUG=1\n\t-Wall\n", "build_flags = \n\t-DDEBUG=1\n\t-Wall\n\t-DFOO=1\n");
        assert_eq!(edited, expected);
    }

    #[test]
    fn set_replaces_a_single_line_value_in_place() {
        let edited = apply_edit(
            MULTI_ENV_INI,
            &IniEdit::Set {
                section: "env".into(),
                name: "monitor_speed".into(),
                values: vec!["9600".into()],
            },
        );
        assert_eq!(edited, MULTI_ENV_INI.replace("monitor_speed = 115200", "monitor_speed = 9600"));
    }

    #[test]
    fn set_on_a_key_that_does_not_exist_yet_inserts_it_after_the_last_entry() {
        let edited = apply_edit(
            MULTI_ENV_INI,
            &IniEdit::Set {
                section: "env:uno".into(),
                name: "framework".into(),
                values: vec!["arduino".into()],
            },
        );
        assert!(edited.contains("[env:uno]\nplatform = atmelavr\nboard = uno\nframework = arduino\n"));
        // everything before the touched section is untouched
        assert!(edited.starts_with(&MULTI_ENV_INI[..MULTI_ENV_INI.find("[env:uno]").unwrap()]));
    }

    #[test]
    fn set_on_a_section_that_does_not_exist_creates_it_with_one_blank_separator_line() {
        let edited = apply_edit(
            MULTI_ENV_INI,
            &IniEdit::Set {
                section: "env:new".into(),
                name: "board".into(),
                values: vec!["esp32dev".into()],
            },
        );
        assert!(edited.ends_with("board = uno\n\n[env:new]\nboard = esp32dev\n"));
        assert!(edited.starts_with(MULTI_ENV_INI));
    }

    #[test]
    fn remove_deletes_the_entrys_lines_only() {
        let edited = apply_edit(
            MULTI_ENV_INI,
            &IniEdit::Remove {
                section: "env:esp32dev".into(),
                name: "build_flags".into(),
            },
        );
        assert!(!edited.contains("build_flags"));
        assert!(!edited.contains("-DDEBUG=1"));
        assert!(edited.contains("framework = arduino\n\n[env:uno]"));
    }

    #[test]
    fn remove_of_a_missing_key_is_a_no_op() {
        assert_eq!(
            apply_edit(
                MULTI_ENV_INI,
                &IniEdit::Remove {
                    section: "env:uno".into(),
                    name: "does_not_exist".into(),
                },
            ),
            MULTI_ENV_INI
        );
    }

    #[test]
    fn add_section_appends_at_eof() {
        let edited = apply_edit(MULTI_ENV_INI, &IniEdit::AddSection { name: "env:new".into() });
        assert!(edited.ends_with("board = uno\n\n[env:new]\n"));
    }

    #[test]
    fn add_section_is_idempotent() {
        let edited = apply_edit(MULTI_ENV_INI, &IniEdit::AddSection { name: "env:uno".into() });
        assert_eq!(edited, MULTI_ENV_INI);
    }

    #[test]
    fn remove_section_deletes_the_header_and_its_body() {
        let edited = apply_edit(MULTI_ENV_INI, &IniEdit::RemoveSection { name: "env:uno".into() });
        assert!(!edited.contains("[env:uno]"));
        assert!(!edited.contains("board = uno"));
        // the blank separator line before `[env:uno]` belongs to the *previous* section's
        // trailing content, not to the removed section's range, so it survives.
        assert!(edited.ends_with("\t-Wall\n\n"));
    }

    #[test]
    fn rename_section_only_touches_the_header_line() {
        let edited = apply_edit(
            MULTI_ENV_INI,
            &IniEdit::RenameSection {
                from: "env:uno".into(),
                to: "env:uno-renamed".into(),
            },
        );
        assert!(edited.contains("[env:uno-renamed]\nplatform = atmelavr\nboard = uno\n"));
        assert!(edited.starts_with(&MULTI_ENV_INI[..MULTI_ENV_INI.find("[env:uno]").unwrap()]));
    }

    #[test]
    fn applying_edits_sequentially_compounds_correctly() {
        let edited = apply_edits(
            MULTI_ENV_INI,
            &[
                IniEdit::Set {
                    section: "env:esp32dev".into(),
                    name: "build_flags".into(),
                    values: vec!["-DDEBUG=1".into(), "-Wall".into(), "-DFOO=1".into()],
                },
                IniEdit::RemoveSection { name: "env:uno".into() },
            ],
        );
        assert!(edited.contains("-DFOO=1"));
        assert!(!edited.contains("[env:uno]"));
    }

    #[test]
    fn the_real_fixture_survives_an_edit_and_a_revert_byte_identical() {
        let raw = include_str!("../../../../tests/fixtures/platformio-real-sample.ini");
        let added = apply_edit(
            raw,
            &IniEdit::Set {
                section: "env:esp32-c6-devkitm-1".into(),
                name: "monitor_speed".into(),
                values: vec!["115200".into()],
            },
        );
        assert_ne!(added, raw);
        let reverted = apply_edit(
            &added,
            &IniEdit::Remove {
                section: "env:esp32-c6-devkitm-1".into(),
                name: "monitor_speed".into(),
            },
        );
        assert_eq!(reverted, raw);
    }
}
