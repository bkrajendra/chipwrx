//! The RAM/Flash size parser (`CLI-CONTRACT.md` §5.1, `FR-BUILD-6`). Grounded against a
//! real captured line in `tests/fixtures/pio-run-success.txt`:
//! `RAM:   [=         ]   6.7% (used 22116 bytes from 327680 bytes)`.

use crate::core::proc::events::SizeUsage;
use regex::Regex;
use std::sync::LazyLock;

static SIZE_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<kind>RAM|Flash):\s+\[.*\]\s+[\d.]+%\s+\(used\s+(?P<used>\d+)\s+bytes\s+from\s+(?P<total>\d+)\s+bytes\)").unwrap());

enum SizeKind {
    Ram,
    Flash,
}

fn parse_size_line(line: &str) -> Option<(SizeKind, u64, u64)> {
    let caps = SIZE_LINE.captures(line)?;
    let kind = if &caps["kind"] == "RAM" { SizeKind::Ram } else { SizeKind::Flash };
    let used = caps["used"].parse().ok()?;
    let total = caps["total"].parse().ok()?;
    Some((kind, used, total))
}

/// A `pio run` size report always prints the `RAM:` line before `Flash:` (confirmed in the
/// captured fixture) — this accumulates both before producing a complete [`SizeUsage`].
/// `ram_delta`/`flash_delta` are left `None`; the caller fills them in once it knows the
/// previous successful build's numbers (`.vibe/builds.json`), which this parser has no
/// access to.
#[derive(Default)]
pub struct SizeParser {
    ram: Option<(u64, u64)>,
    flash: Option<(u64, u64)>,
}

impl SizeParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `Some(SizeUsage)` exactly once — when the `Flash:` line arrives, since the
    /// captured fixture confirms `RAM:` always precedes it. Every other line, including
    /// ones seen after that point, returns `None`.
    pub fn feed_line(&mut self, line: &str) -> Option<SizeUsage> {
        match parse_size_line(line)? {
            (SizeKind::Ram, used, total) => {
                self.ram = Some((used, total));
                None
            }
            (SizeKind::Flash, flash_used, flash_total) => {
                self.flash = Some((flash_used, flash_total));
                let (ram_used, ram_total) = self.ram?;
                Some(SizeUsage {
                    ram_used,
                    ram_total,
                    flash_used,
                    flash_total,
                    ram_delta: None,
                    flash_delta: None,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_real_captured_ram_and_flash_lines() {
        let mut p = SizeParser::new();
        assert!(p.feed_line("RAM:   [=         ]   6.7% (used 22116 bytes from 327680 bytes)").is_none());
        let usage = p.feed_line("Flash: [==        ]  20.9% (used 274536 bytes from 1310720 bytes)").expect("usage");
        assert_eq!(usage.ram_used, 22116);
        assert_eq!(usage.ram_total, 327680);
        assert_eq!(usage.flash_used, 274536);
        assert_eq!(usage.flash_total, 1310720);
        assert_eq!(usage.ram_delta, None);
        assert_eq!(usage.flash_delta, None);
    }

    #[test]
    fn ignores_unrelated_lines() {
        let mut p = SizeParser::new();
        assert!(p.feed_line("Compiling .pio/build/esp32dev/src/main.cpp.o").is_none());
        assert!(p.feed_line("RAM:   [=         ]   6.7% (used 22116 bytes from 327680 bytes)").is_none());
        assert!(p.feed_line("Building .pio/build/esp32dev/firmware.bin").is_none());
    }

    #[test]
    fn full_success_fixture_yields_exactly_one_size_usage() {
        let text = include_str!("../../../../tests/fixtures/pio-run-success.txt");
        let mut p = SizeParser::new();
        let usages: Vec<_> = text.lines().filter_map(|l| p.feed_line(l)).collect();
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].ram_used, 22116);
        assert_eq!(usages[0].flash_used, 274536);
    }
}
