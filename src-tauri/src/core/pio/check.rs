//! `pio check --json-output` (`CLI-CONTRACT.md` §5.2, `FR-BUILD-9`). Argv construction plus
//! the JSON parser, grounded against a real captured run
//! (`tests/fixtures/pio-check-json-real.json` — `eClock`, an ESP32-C6 project, zero
//! defects) and a hand-built fixture with populated defects
//! (`pio-check-json-with-defects.json`) whose field names come from the real
//! `--template=...` string PlatformIO passed to `cppcheck` in that same captured run
//! (`severity`, `message`, `file`, `line`, `column`, `callstack`, `cwe`, `id`), not guessed.

use crate::core::proc::events::{Defect, DefectSource, Severity};
use serde::Deserialize;
use std::path::Path;

pub fn check_args(dir: &Path, env: &str) -> Vec<String> {
    vec![
        "check".into(),
        "-d".into(),
        dir.display().to_string(),
        "-e".into(),
        env.into(),
        "--json-output".into(),
    ]
}

#[derive(Deserialize)]
struct RawCheckEnv {
    #[serde(default)]
    defects: Vec<RawDefect>,
}

#[derive(Deserialize)]
struct RawDefect {
    severity: String,
    message: String,
    file: String,
    line: u32,
    column: Option<u32>,
}

/// `SPEC.md` §8 open question 40: PlatformIO's `low`/`medium`/`high` don't line up with
/// this app's `Severity` (shared with compiler/linker defects) — mapped by blocking vs.
/// advisory vs. informational, not verified against spec text.
fn map_severity(s: &str) -> Severity {
    match s {
        "high" => Severity::Error,
        "medium" => Severity::Warning,
        _ => Severity::Note, // "low" and anything unrecognized
    }
}

/// Parses `pio check --json-output`'s stdout. The real command prints nothing but this one
/// JSON array on success; tolerant of leading/trailing noise (banners, warnings) by trying
/// the whole buffer first, then falling back to the last line that parses — `pio test`'s
/// equivalent output showed library-install banners can precede the JSON in some runs.
pub fn parse_check_json(text: &str) -> Vec<Defect> {
    let envs: Vec<RawCheckEnv> = serde_json::from_str(text.trim())
        .or_else(|_| {
            text.lines()
                .rev()
                .find_map(|line| serde_json::from_str::<Vec<RawCheckEnv>>(line.trim()).ok())
                .ok_or(())
        })
        .unwrap_or_default();

    envs.into_iter()
        .flat_map(|e| e.defects)
        .map(|d| Defect {
            file: d.file,
            line: d.line,
            column: d.column,
            severity: map_severity(&d.severity),
            message: d.message,
            source: DefectSource::Check,
            raw: String::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn check_args_matches_the_cli_contract_shape() {
        let dir = PathBuf::from("/home/fay/greenhouse-sensor");
        assert_eq!(
            check_args(&dir, "esp32dev"),
            vec!["check", "-d", "/home/fay/greenhouse-sensor", "-e", "esp32dev", "--json-output"]
        );
    }

    #[test]
    fn parses_the_real_captured_zero_defect_run() {
        let text = include_str!("../../../../tests/fixtures/pio-check-json-real.json");
        let defects = parse_check_json(text);
        assert!(defects.is_empty());
    }

    #[test]
    fn parses_populated_defects_with_severity_mapping() {
        let text = include_str!("../../../../tests/fixtures/pio-check-json-with-defects.json");
        let defects = parse_check_json(text);
        assert_eq!(defects.len(), 2);
        assert!(matches!(defects[0].severity, Severity::Warning)); // "medium"
        assert!(matches!(defects[1].severity, Severity::Error)); // "high"
        assert!(matches!(defects[0].source, DefectSource::Check));
        assert_eq!(defects[1].line, 88);
        assert_eq!(defects[1].message, "Uninitialized variable: sensorState");
    }

    #[test]
    fn malformed_input_yields_no_defects_rather_than_panicking() {
        assert!(parse_check_json("not json at all").is_empty());
    }
}
