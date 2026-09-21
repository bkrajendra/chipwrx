//! FakeCli — replays a fixture file to stdout/stderr with (optional) per-line timing,
//! substituted for the real `pio`/`claude` binary via the resolved-path setting so the
//! whole pipeline is testable without network, hardware, or cost. See `ARCHITECTURE.md`
//! §10 and `ROADMAP.md` M0.
//!
//! Fixture selection deliberately isn't argv: the whole point of substituting this binary
//! for `pio`/`claude` is that the caller still constructs the *real* tool's argv
//! (`pio --version`, `claude -p "..." --output-format stream-json`, …), which FakeCli
//! makes no attempt to parse. In priority order:
//!
//!   1. `--fixture <path>` / `--interval-ms <n>` / `--exit-code <n>` on argv, for
//!      convenience when running it by hand from a terminal. Any other argv token is
//!      silently ignored — **except** `-o <path>`, which is honored exactly like real
//!      `curl -o`: non-stderr-marked fixture lines are written to that file instead of
//!      stdout, so this binary can also stand in for `curl` in install-flow tests.
//!   2. `FAKE_CLI_FIXTURE` / `FAKE_CLI_INTERVAL_MS` / `FAKE_CLI_EXIT_CODE` env vars, set on
//!      the child via `SpawnSpec.env` — safe for tests that run concurrently, since each
//!      spawned process gets its own environment.
//!   3. A `.fake-cli-fixture` marker file in the process's current directory
//!      (`SpawnSpec.cwd`), first line = fixture path, optional second/third lines =
//!      interval_ms / exit_code. Useful when a caller can't easily set env vars per spawn
//!      but *can* control the spawn's cwd (each test using its own temp cwd).
//!
//! Fixture format: plain UTF-8 text, one output line per line. A line beginning with the
//! literal marker `!ERR!` is written to stderr with the marker stripped; everything else
//! goes to stdout.

use std::env;
use std::fs;
use std::io::{self, BufWriter, Write};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

const STDERR_MARKER: &str = "!ERR!";
const MARKER_FILE: &str = ".fake-cli-fixture";

#[derive(Default)]
struct Selection {
    fixture: Option<String>,
    interval_ms: Option<u64>,
    exit_code: Option<u8>,
    /// curl-style `-o <path>`: write fixture content there instead of stdout.
    output_file: Option<String>,
}

impl Selection {
    fn merge(self, fallback: Selection) -> Selection {
        Selection {
            fixture: self.fixture.or(fallback.fixture),
            interval_ms: self.interval_ms.or(fallback.interval_ms),
            exit_code: self.exit_code.or(fallback.exit_code),
            output_file: self.output_file.or(fallback.output_file),
        }
    }
}

fn from_argv() -> Selection {
    let mut sel = Selection::default();
    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--fixture" => sel.fixture = it.next(),
            "--interval-ms" => sel.interval_ms = it.next().and_then(|v| v.parse().ok()),
            "--exit-code" => sel.exit_code = it.next().and_then(|v| v.parse().ok()),
            "-o" => sel.output_file = it.next(),
            _ => {} // a real pio/claude flag or its value — not ours to understand
        }
    }
    sel
}

fn from_env() -> Selection {
    Selection {
        fixture: env::var("FAKE_CLI_FIXTURE").ok(),
        interval_ms: env::var("FAKE_CLI_INTERVAL_MS").ok().and_then(|v| v.parse().ok()),
        exit_code: env::var("FAKE_CLI_EXIT_CODE").ok().and_then(|v| v.parse().ok()),
        output_file: None,
    }
}

fn from_marker_file() -> Selection {
    let Ok(contents) = fs::read_to_string(MARKER_FILE) else {
        return Selection::default();
    };
    let mut lines = contents.lines();
    Selection {
        fixture: lines.next().map(str::to_string),
        interval_ms: lines.next().and_then(|l| l.parse().ok()),
        exit_code: lines.next().and_then(|l| l.parse().ok()),
        output_file: None,
    }
}

fn main() -> ExitCode {
    let selection = from_argv().merge(from_env()).merge(from_marker_file());
    let interval_ms = selection.interval_ms.unwrap_or(0);
    let exit_code = selection.exit_code.unwrap_or(0);

    let Some(fixture) = selection.fixture else {
        eprintln!(
            "fake_cli: no fixture given (set FAKE_CLI_FIXTURE, pass --fixture, or write {MARKER_FILE})"
        );
        return ExitCode::from(2);
    };

    let content = match fs::read_to_string(&fixture) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("fake_cli: reading fixture {fixture}: {e}");
            return ExitCode::from(2);
        }
    };

    let stderr = io::stderr();
    let mut err = BufWriter::new(stderr.lock());

    let mut out_file = selection
        .output_file
        .as_ref()
        .map(|path| fs::File::create(path).expect("fake_cli: creating -o output file"));
    let stdout = io::stdout();
    let mut stdout_lock = BufWriter::new(stdout.lock());

    for line in content.lines() {
        if let Some(rest) = line.strip_prefix(STDERR_MARKER) {
            let _ = writeln!(err, "{rest}");
            let _ = err.flush();
        } else if let Some(f) = out_file.as_mut() {
            let _ = writeln!(f, "{line}");
        } else {
            let _ = writeln!(stdout_lock, "{line}");
            let _ = stdout_lock.flush();
        }
        if interval_ms > 0 {
            thread::sleep(Duration::from_millis(interval_ms));
        }
    }

    ExitCode::from(exit_code)
}
