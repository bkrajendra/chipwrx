//! Parses `pio project config --lint` (`FR-INI-5`) — **not** `--lint --json-output`, which
//! emits a Python `repr` (single-quoted, invalid JSON — `CLI-CONTRACT.md` §4.3). This
//! parses the human-readable table form the CLI contract recommends instead.
//!
//! Grounded in three real captures against a physical project this session: a clean file
//! (`pio-project-config-lint-clean-real.txt`), an invalid-choice-value error plus an
//! unknown-option warning (`pio-project-config-lint-errors-real.txt`), and a genuine INI
//! syntax error (captured inline in this module's tests) — which revealed the `source`
//! (`path:line`) suffix CLI-CONTRACT describes is only present for *some* error types
//! (parse errors), not others (e.g. an invalid option value has no line to point at).

use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LintError {
    /// The Python exception class name, e.g. `"ProjectOptionValueError"`, `"ParsingError"`.
    pub kind: String,
    pub message: String,
    /// `"path:line"` when the CLI included one — only some error kinds do (see module doc).
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LintReport {
    pub errors: Vec<LintError>,
    pub warnings: Vec<String>,
}

const CLEAN_MARKER: &str = "free from linting errors";

static TRAILING_SOURCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?P<message>.*)\s{2,}(?P<source>\S+:\d+)$").unwrap());

fn split_leading_kind(line: &str) -> Option<(&str, &str)> {
    let idx = line.find("  ")?;
    let kind = line[..idx].trim();
    let rest = line[idx..].trim_start();
    if kind.is_empty() || rest.is_empty() {
        return None;
    }
    Some((kind, rest))
}

fn split_trailing_source(rest: &str) -> (String, Option<String>) {
    match TRAILING_SOURCE.captures(rest) {
        Some(caps) => (caps["message"].to_string(), Some(caps["source"].to_string())),
        None => (rest.to_string(), None),
    }
}

/// Parses `pio project config --lint`'s plain-text output. Tolerant: a line that doesn't
/// match the `<kind>  <message>` shape (extra blank lines, a differently-worded clean
/// message in a future Core version) is simply skipped rather than erroring — a report
/// with zero errors and zero warnings degrades gracefully to "nothing to show," not a
/// crash.
pub fn parse_lint_output(text: &str) -> LintReport {
    let mut report = LintReport::default();
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.contains(CLEAN_MARKER) {
            continue;
        }
        let Some((kind, rest)) = split_leading_kind(line) else { continue };
        if kind == "Warning" {
            report.warnings.push(rest.to_string());
        } else {
            let (message, source) = split_trailing_source(rest);
            report.errors.push(LintError {
                kind: kind.to_string(),
                message,
                source,
            });
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_file_yields_an_empty_report() {
        let text = include_str!("../../../../tests/fixtures/pio-project-config-lint-clean-real.txt");
        let report = parse_lint_output(text);
        assert!(report.errors.is_empty());
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn real_captured_invalid_choice_and_unknown_option_parse_correctly() {
        let text = include_str!("../../../../tests/fixtures/pio-project-config-lint-errors-real.txt");
        let report = parse_lint_output(text);

        assert_eq!(report.errors.len(), 1);
        let err = &report.errors[0];
        assert_eq!(err.kind, "ProjectOptionValueError");
        assert!(err.message.contains("not_a_real_mode"));
        assert!(err.message.contains("lib_ldf_mode"));
        // this error kind has no `path:line` suffix in the real capture
        assert_eq!(err.source, None);

        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("totally_bogus_option"));
    }

    #[test]
    fn a_genuine_parse_error_carries_a_path_line_source() {
        // Captured live this session: `pio project config --lint` against a file with a
        // line that has neither `=` nor a preceding `[section]`.
        let line = "ParsingError  Parsing error: 'this line has no equals sign and no section'  C:\\Users\\resea\\AppData\\Local\\Temp\\vibe-hw-m7-ini\\platformio.ini:8";
        let report = parse_lint_output(line);
        assert_eq!(report.errors.len(), 1);
        let err = &report.errors[0];
        assert_eq!(err.kind, "ParsingError");
        assert!(err.message.starts_with("Parsing error:"));
        assert_eq!(err.source.as_deref(), Some("C:\\Users\\resea\\AppData\\Local\\Temp\\vibe-hw-m7-ini\\platformio.ini:8"));
    }

    #[test]
    fn blank_lines_and_unrecognized_text_are_skipped_not_fatal() {
        let report = parse_lint_output("\n\nsome unrelated line with no double space\n");
        assert!(report.errors.is_empty());
        assert!(report.warnings.is_empty());
    }
}
