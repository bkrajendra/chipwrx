//! Semver-ish parsing and comparison for `claude --version` / `pio --version` output, and
//! the feature-gate table (`TOOLCHAIN-SETUP.md` §4.3).

use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self { major, minor, patch }
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Extracts the first `^(\d+)\.(\d+)\.(\d+)` match anywhere in `text` — matches
/// `CLI-CONTRACT.md`'s `claude --version` rule and works just as well against PlatformIO's
/// `"PlatformIO Core, version 6.2.0"`.
pub fn parse_version(text: &str) -> Option<Version> {
    let bytes = text.as_bytes();
    for start in 0..bytes.len() {
        if let Some(v) = try_parse_at(bytes, start) {
            return Some(v);
        }
    }
    None
}

fn try_parse_at(bytes: &[u8], start: usize) -> Option<Version> {
    let (major, next) = take_digits(bytes, start)?;
    let next = expect_dot(bytes, next)?;
    let (minor, next) = take_digits(bytes, next)?;
    let next = expect_dot(bytes, next)?;
    let (patch, _next) = take_digits(bytes, next)?;
    Some(Version::new(major, minor, patch))
}

fn take_digits(bytes: &[u8], start: usize) -> Option<(u32, usize)> {
    let mut end = start;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == start {
        return None;
    }
    std::str::from_utf8(&bytes[start..end])
        .ok()?
        .parse::<u32>()
        .ok()
        .map(|v| (v, end))
}

fn expect_dot(bytes: &[u8], pos: usize) -> Option<usize> {
    if bytes.get(pos) == Some(&b'.') {
        Some(pos + 1)
    } else {
        None
    }
}

pub fn meets_minimum(found: Version, minimum: Version) -> bool {
    found.cmp(&minimum) != Ordering::Less
}

/// A capability gated behind a Claude Code version, feature-detected from `capabilities[]`
/// on `system/init` first and falling back to version comparison only when no capability
/// string covers it (`TOOLCHAIN-SETUP.md` §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeFeature {
    PermissionPromptsNone,
    CapabilitiesOnInit,
    NestedSubagentMessages,
    ResumeFromAnyDirectory,
}

impl ClaudeFeature {
    /// The capability string this feature reports as, when the running CLI is new enough
    /// to include `capabilities[]` at all.
    pub fn capability_name(self) -> Option<&'static str> {
        match self {
            ClaudeFeature::PermissionPromptsNone => Some("permission-prompts-none"),
            ClaudeFeature::NestedSubagentMessages => Some("nested-subagent-messages"),
            // These two are never reported in capabilities[] itself — see version fallback.
            ClaudeFeature::CapabilitiesOnInit | ClaudeFeature::ResumeFromAnyDirectory => None,
        }
    }

    fn version_floor(self) -> Version {
        match self {
            ClaudeFeature::PermissionPromptsNone => Version::new(2, 1, 259),
            ClaudeFeature::CapabilitiesOnInit => Version::new(2, 1, 205),
            ClaudeFeature::NestedSubagentMessages => Version::new(2, 1, 219),
            ClaudeFeature::ResumeFromAnyDirectory => Version::new(2, 1, 223),
        }
    }
}

/// `capabilities` is `None` when the running CLI predates `capabilities[]` entirely
/// (< 2.1.205) — in that case every feature falls back to version comparison.
pub fn claude_supports(
    feature: ClaudeFeature,
    version: Version,
    capabilities: Option<&[String]>,
) -> bool {
    if let (Some(caps), Some(name)) = (capabilities, feature.capability_name()) {
        return caps.iter().any(|c| c == name);
    }
    meets_minimum(version, feature.version_floor())
}

pub const CLAUDE_MINIMUM: Version = Version::new(2, 1, 0);
pub const PIO_MINIMUM: Version = Version::new(6, 1, 0);
pub const PYTHON_MINIMUM: Version = Version::new(3, 6, 0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_claude_version_output() {
        assert_eq!(
            parse_version("2.1.211 (Claude Code)"),
            Some(Version::new(2, 1, 211))
        );
    }

    #[test]
    fn parses_pio_version_output() {
        assert_eq!(
            parse_version("PlatformIO Core, version 6.2.0"),
            Some(Version::new(6, 2, 0))
        );
    }

    #[test]
    fn parses_python_version_output() {
        assert_eq!(parse_version("Python 3.11.4"), Some(Version::new(3, 11, 4)));
    }

    #[test]
    fn no_version_found_returns_none() {
        assert_eq!(parse_version("command not found"), None);
    }

    #[test]
    fn ordering_and_minimum_check() {
        assert!(meets_minimum(Version::new(2, 1, 259), CLAUDE_MINIMUM));
        assert!(!meets_minimum(Version::new(1, 9, 9), CLAUDE_MINIMUM));
        assert!(meets_minimum(Version::new(6, 1, 0), PIO_MINIMUM));
        assert!(!meets_minimum(Version::new(6, 0, 9), PIO_MINIMUM));
    }

    #[test]
    fn feature_gate_prefers_capability_string_over_version() {
        // Version says no, but the capability string says yes — capability wins.
        let caps = vec!["permission-prompts-none".to_string()];
        assert!(claude_supports(
            ClaudeFeature::PermissionPromptsNone,
            Version::new(2, 1, 100),
            Some(&caps),
        ));
    }

    #[test]
    fn feature_gate_falls_back_to_version_when_capabilities_absent() {
        assert!(claude_supports(
            ClaudeFeature::PermissionPromptsNone,
            Version::new(2, 1, 259),
            None,
        ));
        assert!(!claude_supports(
            ClaudeFeature::PermissionPromptsNone,
            Version::new(2, 1, 100),
            None,
        ));
    }

    #[test]
    fn feature_gate_falls_back_to_version_when_capability_list_present_but_silent() {
        // capabilities[] exists (CLI is >= 2.1.205) but doesn't mention this feature —
        // still means "no", not "fall back to version".
        let caps: Vec<String> = vec![];
        assert!(!claude_supports(
            ClaudeFeature::PermissionPromptsNone,
            Version::new(2, 1, 300),
            Some(&caps),
        ));
    }

    #[test]
    fn features_with_no_capability_string_always_use_version() {
        assert!(claude_supports(
            ClaudeFeature::ResumeFromAnyDirectory,
            Version::new(2, 1, 223),
            Some(&["something-else".to_string()]),
        ));
        assert!(!claude_supports(
            ClaudeFeature::ResumeFromAnyDirectory,
            Version::new(2, 1, 222),
            Some(&["something-else".to_string()]),
        ));
    }
}
