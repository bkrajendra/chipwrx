//! The build/link diagnostic parser (`CLI-CONTRACT.md` §5.1, `FR-BUILD-4`). Grounded
//! against real captured `pio run` output in `tests/fixtures/pio-run-{success,fail}.txt`.
//!
//! Two forms, fed one line at a time (stdout+stderr combined, in arrival order):
//!
//! 1. The compiler form — self-contained on one line:
//!    `src/main.cpp:5:11: error: expected primary-expression before ';' token`
//! 2. The linker's `undefined reference` form. GNU ld emits this as a `file:line:` prefix
//!    followed by `undefined reference to \`sym'` — usually on the *same* line
//!    (`src/main.cpp:12: undefined reference to \`missingFunction()'`), but CLI-CONTRACT.md
//!    §5.1 separately calls out "the preceding `file:line:` line," which is unverified
//!    against a real captured linker failure (`SPEC.md` §8 open question 16) — this parser
//!    handles both: a combined line, and a bare `file:line:` line (no severity word) whose
//!    file/line is reused if an `undefined reference` with no prefix of its own follows.

use crate::core::proc::events::{Defect, DefectSource, Severity};
use regex::Regex;
use std::sync::LazyLock;

static COMPILER_FORM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<file>[^\s:]+):(?P<line>\d+):(?:(?P<col>\d+):)?\s+(?P<sev>error|warning|note|fatal error):\s+(?P<msg>.*)$").unwrap()
});

/// `file:line:` followed directly by the undefined-reference message on the same line.
static LINKER_COMBINED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<file>[^\s:]+):(?P<line>\d+):\s*undefined reference to\s*[`'‘](?P<sym>[^'’]+)['’]").unwrap());

/// An `undefined reference` with no `file:line:` prefix of its own — attributed to the
/// most recently seen bare `file:line:` line, if any.
static LINKER_BARE_SYMBOL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"undefined reference to\s*[`'‘](?P<sym>[^'’]+)['’]").unwrap());

/// A bare `file:line:` prefix with no recognized severity word after it — tracked as
/// context for the next line, in case that turns out to be a linker error with no prefix
/// of its own.
static BARE_FILE_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?P<file>[^\s:]+):(?P<line>\d+):").unwrap());

fn severity_of(word: &str) -> Severity {
    match word {
        "warning" => Severity::Warning,
        "note" => Severity::Note,
        _ => Severity::Error, // "error" | "fatal error"
    }
}

/// Stateful across a build's output — the linker fallback form needs to remember the
/// previous line's `file:line:` prefix.
#[derive(Default)]
pub struct DiagnosticParser {
    pending_file_line: Option<(String, u32)>,
}

impl DiagnosticParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a `Defect` if `line` matches a recognized diagnostic form, `None`
    /// otherwise (the overwhelming majority of build output — compiling/linking progress,
    /// package banners — never matches and that's expected, not an error).
    pub fn feed_line(&mut self, line: &str) -> Option<Defect> {
        if let Some(caps) = COMPILER_FORM.captures(line) {
            self.pending_file_line = None;
            return Some(Defect {
                file: caps["file"].to_string(),
                line: caps["line"].parse().unwrap_or(0),
                column: caps.name("col").and_then(|m| m.as_str().parse().ok()),
                severity: severity_of(&caps["sev"]),
                message: caps["msg"].to_string(),
                source: DefectSource::Compiler,
                raw: line.to_string(),
            });
        }

        if let Some(caps) = LINKER_COMBINED.captures(line) {
            self.pending_file_line = None;
            return Some(Defect {
                file: caps["file"].to_string(),
                line: caps["line"].parse().unwrap_or(0),
                column: None,
                severity: Severity::Error,
                message: format!("undefined reference to `{}'", &caps["sym"]),
                source: DefectSource::Linker,
                raw: line.to_string(),
            });
        }

        if let Some(caps) = LINKER_BARE_SYMBOL.captures(line) {
            if let Some((file, defect_line)) = self.pending_file_line.take() {
                return Some(Defect {
                    file,
                    line: defect_line,
                    column: None,
                    severity: Severity::Error,
                    message: format!("undefined reference to `{}'", &caps["sym"]),
                    source: DefectSource::Linker,
                    raw: line.to_string(),
                });
            }
            // No prior file:line context — nothing useful to attribute this to.
            return None;
        }

        if let Some(caps) = BARE_FILE_LINE.captures(line) {
            self.pending_file_line = Some((caps["file"].to_string(), caps["line"].parse().unwrap_or(0)));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_real_captured_compiler_error() {
        let mut p = DiagnosticParser::new();
        let d = p
            .feed_line("src/main.cpp:5:11: error: expected primary-expression before ';' token")
            .expect("defect");
        assert_eq!(d.file, "src/main.cpp");
        assert_eq!(d.line, 5);
        assert_eq!(d.column, Some(11));
        assert!(matches!(d.severity, Severity::Error));
        assert_eq!(d.message, "expected primary-expression before ';' token");
        assert!(matches!(d.source, DefectSource::Compiler));
    }

    #[test]
    fn parses_a_warning() {
        let mut p = DiagnosticParser::new();
        let d = p.feed_line("src/main.cpp:12:3: warning: unused variable 'x' [-Wunused-variable]").expect("defect");
        assert!(matches!(d.severity, Severity::Warning));
    }

    #[test]
    fn parses_fatal_error_as_error_severity() {
        let mut p = DiagnosticParser::new();
        let d = p.feed_line("src/main.cpp:1:10: fatal error: missing.h: No such file or directory").expect("defect");
        assert!(matches!(d.severity, Severity::Error));
    }

    #[test]
    fn ordinary_build_progress_lines_produce_no_defect() {
        let mut p = DiagnosticParser::new();
        assert!(p.feed_line("Compiling .pio/build/esp32dev/src/main.cpp.o").is_none());
        assert!(p.feed_line("Linking .pio/build/esp32dev/firmware.elf").is_none());
        assert!(p.feed_line("PLATFORM: Espressif 32 (55.3.311)").is_none());
    }

    #[test]
    fn parses_a_linker_error_with_file_line_and_symbol_on_one_line() {
        let mut p = DiagnosticParser::new();
        let d = p.feed_line("src/main.cpp:12: undefined reference to `missingFunction()'").expect("defect");
        assert_eq!(d.file, "src/main.cpp");
        assert_eq!(d.line, 12);
        assert!(matches!(d.source, DefectSource::Linker));
        assert!(d.message.contains("missingFunction()"));
    }

    #[test]
    fn attributes_a_bare_undefined_reference_to_the_preceding_file_line() {
        let mut p = DiagnosticParser::new();
        assert!(p.feed_line(".pio/build/esp32dev/src/main.cpp.o: In function `loop()':").is_none());
        // A bare `file:line:` context line (no severity word).
        assert!(p.feed_line("src/main.cpp:12:").is_none());
        let d = p.feed_line("undefined reference to `missingFunction()'").expect("defect");
        assert_eq!(d.file, "src/main.cpp");
        assert_eq!(d.line, 12);
        assert!(matches!(d.source, DefectSource::Linker));
    }

    #[test]
    fn a_bare_undefined_reference_with_no_preceding_context_is_dropped_not_fatal() {
        let mut p = DiagnosticParser::new();
        assert!(p.feed_line("undefined reference to `missingFunction()'").is_none());
    }

    #[test]
    fn full_fixture_produces_exactly_the_two_captured_errors() {
        let text = include_str!("../../../../tests/fixtures/pio-run-fail.txt");
        let mut p = DiagnosticParser::new();
        let defects: Vec<_> = text.lines().filter_map(|l| p.feed_line(l)).collect();
        assert_eq!(defects.len(), 2, "{defects:?}");
        assert_eq!(defects[0].line, 5);
        assert_eq!(defects[1].line, 9);
        assert!(defects.iter().all(|d| matches!(d.severity, Severity::Error)));
    }

    #[test]
    fn full_success_fixture_produces_no_defects() {
        let text = include_str!("../../../../tests/fixtures/pio-run-success.txt");
        let mut p = DiagnosticParser::new();
        let defects: Vec<_> = text.lines().filter_map(|l| p.feed_line(l)).collect();
        assert!(defects.is_empty(), "{defects:?}");
    }
}
