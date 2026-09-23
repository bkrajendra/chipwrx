//! Format-preserving `platformio.ini` parser (`FR-INI-4`, `ARCHITECTURE.md` §3
//! `core/ini::read_declared`). Every original line is kept verbatim in [`IniDoc::lines`] —
//! parsing only *categorizes* line ranges into sections/entries, it never rewrites them.
//! That's what makes [`super::patch`]'s surgical edits byte-identical everywhere except the
//! lines actually touched.
//!
//! Grounded in real `platformio.ini` files captured against a physical
//! ESP32-C6-DevKitM-1 project this session (`tests/fixtures/platformio-real-sample.ini`,
//! `platformio-real-multienv.ini`) — including the exact multi-value continuation shape
//! PlatformIO's own writer produces (`key = ` with a **trailing space**, then tab-indented
//! continuation lines — `DATA-MODEL.md` §8.1's own example omits that trailing space;
//! `SPEC.md` §8 open question 30 corrects it from the real capture).

#[derive(Debug, Clone, PartialEq, Default)]
pub struct IniDoc {
    /// Every source line, each still carrying its own original line ending (or none, for a
    /// final line with no trailing newline). `lines.concat()` always reproduces the exact
    /// original text.
    pub lines: Vec<String>,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// `""` for the implicit preamble before the first `[section]` header (top-of-file
    /// comments, mostly) — never user-addressable by that name.
    pub name: String,
    /// Line index of the `[name]` line itself; `None` for the preamble.
    pub header_line: Option<usize>,
    /// First line index belonging to this section's body.
    pub start: usize,
    /// Exclusive end — the next section's `header_line`, or `lines.len()`.
    pub end: usize,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    /// The `key = ...` line index.
    pub start: usize,
    /// Exclusive — `start + 1 + continuation_line_count`.
    pub end: usize,
    /// One trimmed string per contributing line: the `key = ` line's own trailing content
    /// (if any) first, then each indented continuation line in order. A single-line
    /// `key = value` entry has exactly one element.
    pub values: Vec<String>,
}

impl IniDoc {
    pub fn raw(&self) -> String {
        self.lines.concat()
    }

    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }
}

impl Section {
    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }
}

/// Splits `text` into lines, each still carrying its own line ending (`"\r\n"` or `"\n"`);
/// the final line carries none if the original text didn't end in one. Unlike
/// `str::lines()`, this never discards the exact bytes needed to reconstruct `text` via
/// `.concat()`.
pub(super) fn split_lines_keep_ends(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            out.push(text[start..=i].to_string());
            start = i + 1;
        }
        i += 1;
    }
    if start < text.len() {
        out.push(text[start..].to_string());
    }
    out
}

pub(super) fn strip_eol(line: &str) -> &str {
    line.strip_suffix("\r\n").or_else(|| line.strip_suffix('\n')).unwrap_or(line)
}

/// The file's predominant line ending, for lines this module writes itself. Defaults to
/// `"\n"` for an empty document or one with no line endings at all.
pub(super) fn detect_eol(lines: &[String]) -> &'static str {
    for line in lines {
        if line.ends_with("\r\n") {
            return "\r\n";
        }
        if line.ends_with('\n') {
            return "\n";
        }
    }
    "\n"
}

fn parse_section_header(trimmed: &str) -> Option<String> {
    let inner = trimmed.strip_prefix('[')?;
    let inner = inner.strip_suffix(']')?;
    Some(inner.to_string())
}

/// Parses `raw` into a format-preserving [`IniDoc`]. Never fails — an unparseable line
/// (no `=`, not blank/comment/section) is simply not modeled as an entry, but its exact
/// text is still kept in `lines` and still round-trips.
pub fn parse(raw: &str) -> IniDoc {
    let lines = split_lines_keep_ends(raw);
    let mut sections = Vec::new();

    let mut cur_name = String::new();
    let mut cur_header_line: Option<usize> = None;
    let mut cur_start = 0usize;
    let mut cur_entries: Vec<Entry> = Vec::new();

    let mut i = 0usize;
    while i < lines.len() {
        let content = strip_eol(&lines[i]);
        let trimmed = content.trim_start();

        if trimmed.starts_with('[') {
            if let Some(name) = parse_section_header(trimmed) {
                sections.push(Section {
                    name: std::mem::take(&mut cur_name),
                    header_line: cur_header_line,
                    start: cur_start,
                    end: i,
                    entries: std::mem::take(&mut cur_entries),
                });
                cur_name = name;
                cur_header_line = Some(i);
                cur_start = i + 1;
                i += 1;
                continue;
            }
        }

        let is_blank_comment_or_indented =
            trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') || content.starts_with(' ') || content.starts_with('\t');
        if is_blank_comment_or_indented {
            i += 1;
            continue;
        }

        if let Some(eq_pos) = trimmed.find('=') {
            let key = trimmed[..eq_pos].trim().to_string();
            let first_value = trimmed[eq_pos + 1..].trim().to_string();
            let mut values = Vec::new();
            if !first_value.is_empty() {
                values.push(first_value);
            }
            let entry_start = i;
            i += 1;
            while i < lines.len() {
                let c = strip_eol(&lines[i]);
                if c.trim().is_empty() {
                    break; // a blank line ends the continuation, and is not consumed
                }
                if !(c.starts_with(' ') || c.starts_with('\t')) {
                    break;
                }
                let v = c.trim().to_string();
                if !v.is_empty() {
                    values.push(v);
                }
                i += 1;
            }
            cur_entries.push(Entry {
                name: key,
                start: entry_start,
                end: i,
                values,
            });
            continue;
        }

        i += 1; // unparseable line — kept in `lines`, just not modeled as an entry
    }

    sections.push(Section {
        name: cur_name,
        header_line: cur_header_line,
        start: cur_start,
        end: lines.len(),
        entries: cur_entries,
    });

    IniDoc { lines, sections }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_lines_keep_ends_preserves_lf_and_a_final_line_with_no_newline() {
        let lines = split_lines_keep_ends("a\nb\nc");
        assert_eq!(lines, vec!["a\n", "b\n", "c"]);
        assert_eq!(lines.concat(), "a\nb\nc");
    }

    #[test]
    fn split_lines_keep_ends_preserves_crlf() {
        let lines = split_lines_keep_ends("a\r\nb\r\n");
        assert_eq!(lines, vec!["a\r\n", "b\r\n"]);
        assert_eq!(lines.concat(), "a\r\nb\r\n");
    }

    #[test]
    fn empty_input_yields_no_lines() {
        assert!(split_lines_keep_ends("").is_empty());
    }

    #[test]
    fn round_trips_the_real_captured_single_value_fixture() {
        let raw = include_str!("../../../../tests/fixtures/platformio-real-sample.ini");
        let doc = parse(raw);
        assert_eq!(doc.raw(), raw);
    }

    #[test]
    fn round_trips_the_real_captured_multienv_fixture() {
        let raw = include_str!("../../../../tests/fixtures/platformio-real-multienv.ini");
        let doc = parse(raw);
        assert_eq!(doc.raw(), raw);
    }

    #[test]
    fn parses_declared_sections_and_simple_entries() {
        let raw = "[platformio]\ndefault_envs = esp32dev\n\n[env:esp32dev]\nboard = esp32dev\n";
        let doc = parse(raw);
        assert_eq!(doc.sections.len(), 3); // preamble ("") + platformio + env:esp32dev
        let platformio = doc.section("platformio").expect("platformio section");
        assert_eq!(platformio.entry("default_envs").unwrap().values, vec!["esp32dev"]);
        let env = doc.section("env:esp32dev").expect("env section");
        assert_eq!(env.entry("board").unwrap().values, vec!["esp32dev"]);
    }

    #[test]
    fn parses_multiline_continuation_values() {
        let raw = "[env:x]\nlib_deps = \n\tbblanchon/ArduinoJson@^7.0.0\n\tknolleary/PubSubClient@^2.8\n";
        let doc = parse(raw);
        let entry = doc.section("env:x").unwrap().entry("lib_deps").unwrap();
        assert_eq!(entry.values, vec!["bblanchon/ArduinoJson@^7.0.0", "knolleary/PubSubClient@^2.8"]);
        assert_eq!(entry.start, 1);
        assert_eq!(entry.end, 4);
    }

    #[test]
    fn a_blank_line_ends_a_continuation_without_being_consumed() {
        let raw = "[env:x]\nlib_deps = \n\tfoo\n\nmonitor_speed = 9600\n";
        let doc = parse(raw);
        let section = doc.section("env:x").unwrap();
        assert_eq!(section.entry("lib_deps").unwrap().values, vec!["foo"]);
        assert_eq!(section.entry("monitor_speed").unwrap().values, vec!["9600"]);
        // the blank line (index 3) still exists, untouched, in the raw reconstruction
        assert_eq!(doc.raw(), raw);
    }

    #[test]
    fn comments_and_blank_lines_are_preserved_but_not_modeled_as_entries() {
        let raw = "; header comment\n\n[env:x]\n; a comment inside the section\nboard = esp32dev\n";
        let doc = parse(raw);
        assert_eq!(doc.raw(), raw);
        let env = doc.section("env:x").unwrap();
        assert_eq!(env.entries.len(), 1);
        assert_eq!(env.entries[0].name, "board");
    }

    #[test]
    fn preamble_content_before_the_first_section_is_kept_and_unnamed() {
        let raw = "; top of file\n\n[env:x]\nboard = esp32dev\n";
        let doc = parse(raw);
        let preamble = doc.section("").expect("preamble section");
        assert_eq!(preamble.header_line, None);
        assert!(preamble.entries.is_empty());
        assert_eq!(doc.raw(), raw);
    }

    #[test]
    fn a_file_with_no_sections_at_all_is_one_preamble_section() {
        let raw = "; just comments\n; nothing else\n";
        let doc = parse(raw);
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].name, "");
        assert_eq!(doc.raw(), raw);
    }

    #[test]
    fn an_unparseable_line_is_kept_but_not_modeled() {
        let raw = "[env:x]\nthis has no equals sign\nboard = esp32dev\n";
        let doc = parse(raw);
        assert_eq!(doc.raw(), raw);
        let env = doc.section("env:x").unwrap();
        assert_eq!(env.entries.len(), 1);
        assert_eq!(env.entries[0].name, "board");
    }

    #[test]
    fn detect_eol_prefers_crlf_when_present() {
        assert_eq!(detect_eol(&["a\r\n".to_string(), "b\n".to_string()]), "\r\n");
        assert_eq!(detect_eol(&["a\n".to_string()]), "\n");
        assert_eq!(detect_eol(&["a".to_string()]), "\n");
        assert_eq!(detect_eol(&[]), "\n");
    }
}
