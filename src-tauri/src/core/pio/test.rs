//! `pio test --json-output` (`CLI-CONTRACT.md` §5.3, `FR-BUILD-10`). Argv construction plus
//! the JSON parser, grounded against a real captured run
//! (`tests/fixtures/pio-test-json-real.json` — `eClock`, a project with no actual test
//! files, status `ERRORED`) and a hand-built fixture exercising `PASSED`/`FAILED`/`SKIPPED`
//! (`pio-test-json-mixed.json`) — see `SPEC.md` §8 open question 39.

use crate::core::proc::events::{TestCaseResult, TestStatus, TestSuite};
use serde::Deserialize;
use std::path::Path;

/// `port` covers both `--upload-port` and `--test-port` (`CLI-CONTRACT.md` §5.3: "note
/// `--test-port` is separate from `--upload-port`") — this app tracks one preferred port
/// per workspace, so both point at it, matching `pio_run::upload_args`'s single-port model.
pub fn test_args(dir: &Path, env: &str, port: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "test".into(),
        "-d".into(),
        dir.display().to_string(),
        "-e".into(),
        env.into(),
        "--json-output".into(),
    ];
    if let Some(p) = port {
        args.push("--upload-port".into());
        args.push(p.into());
        args.push("--test-port".into());
        args.push(p.into());
    }
    args
}

#[derive(Deserialize)]
struct RawReport {
    #[serde(default)]
    test_suites: Vec<RawSuite>,
}

#[derive(Deserialize)]
struct RawSuite {
    env_name: String,
    test_name: String,
    status: String,
    duration: f64,
    #[serde(default)]
    test_cases: Vec<RawCase>,
}

#[derive(Deserialize)]
struct RawCase {
    name: String,
    status: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    duration: f64,
    #[serde(default)]
    exception: Option<String>,
    #[serde(default)]
    source: Option<RawSource>,
}

#[derive(Deserialize)]
struct RawSource {
    file: String,
    line: u32,
}

fn map_status(s: &str) -> TestStatus {
    match s {
        "PASSED" => TestStatus::Passed,
        "FAILED" => TestStatus::Failed,
        "SKIPPED" => TestStatus::Skipped,
        _ => TestStatus::Errored, // "ERRORED" and anything unrecognized
    }
}

/// Parses `pio test --json-output`'s stdout — a single JSON object, printed after any
/// build/library-install banner lines (the real captured run has 14 lines of banner before
/// it), so this scans from the end for the last line that parses as the envelope, rather
/// than requiring the whole buffer to be exactly one JSON document the way `pio check`'s
/// (quieter) output usually is.
pub fn parse_test_json(text: &str) -> Vec<TestSuite> {
    let report: Option<RawReport> = serde_json::from_str(text.trim()).ok().or_else(|| {
        text.lines()
            .rev()
            .find_map(|line| serde_json::from_str::<RawReport>(line.trim()).ok())
    });

    let Some(report) = report else {
        return Vec::new();
    };

    report
        .test_suites
        .into_iter()
        .map(|s| TestSuite {
            env_name: s.env_name,
            test_name: s.test_name,
            status: map_status(&s.status),
            duration: s.duration,
            cases: s
                .test_cases
                .into_iter()
                .map(|c| TestCaseResult {
                    name: c.name,
                    status: map_status(&c.status),
                    message: c.message.or(c.exception),
                    duration: c.duration,
                    file: c.source.as_ref().map(|s| s.file.clone()),
                    line: c.source.as_ref().map(|s| s.line),
                })
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_args_without_a_port_omits_the_flags() {
        let dir = PathBuf::from("/home/fay/greenhouse-sensor");
        let args = test_args(&dir, "esp32dev", None);
        assert_eq!(args, vec!["test", "-d", "/home/fay/greenhouse-sensor", "-e", "esp32dev", "--json-output"]);
    }

    #[test]
    fn test_args_with_a_port_sets_both_upload_and_test_port() {
        let dir = PathBuf::from("/home/fay/greenhouse-sensor");
        let args = test_args(&dir, "esp32dev", Some("/dev/ttyUSB0"));
        assert_eq!(
            args,
            vec![
                "test", "-d", "/home/fay/greenhouse-sensor", "-e", "esp32dev", "--json-output", "--upload-port",
                "/dev/ttyUSB0", "--test-port", "/dev/ttyUSB0"
            ]
        );
    }

    #[test]
    fn parses_the_real_captured_errored_run() {
        let text = include_str!("../../../../tests/fixtures/pio-test-json-real.json");
        let suites = parse_test_json(text);
        assert_eq!(suites.len(), 1);
        assert!(matches!(suites[0].status, TestStatus::Errored));
        assert_eq!(suites[0].env_name, "esp32-c6-devkitm-1");
        assert_eq!(suites[0].cases.len(), 1);
        assert!(suites[0].cases[0].message.as_deref().unwrap().contains("Building stage has failed"));
    }

    #[test]
    fn parses_a_mixed_pass_fail_skip_run_with_source_location() {
        let text = include_str!("../../../../tests/fixtures/pio-test-json-mixed.json");
        let suites = parse_test_json(text);
        assert_eq!(suites.len(), 1);
        let cases = &suites[0].cases;
        assert_eq!(cases.len(), 3);
        assert!(matches!(cases[0].status, TestStatus::Passed));
        assert!(matches!(cases[1].status, TestStatus::Failed));
        assert_eq!(cases[1].file.as_deref(), Some("test/test_sensors/test_sensors.cpp"));
        assert_eq!(cases[1].line, Some(22));
        assert!(matches!(cases[2].status, TestStatus::Skipped));
    }

    #[test]
    fn banner_lines_before_the_json_are_skipped() {
        let json = include_str!("../../../../tests/fixtures/pio-test-json-real.json");
        let with_banner = format!("Collected 1 tests\nProcessing esp32-c6-devkitm-1\n{json}");
        assert_eq!(parse_test_json(&with_banner).len(), 1);
    }

    #[test]
    fn malformed_input_yields_no_suites_rather_than_panicking() {
        assert!(parse_test_json("not json at all").is_empty());
    }
}
