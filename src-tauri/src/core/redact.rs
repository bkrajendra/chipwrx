//! Secret redaction. `ARCHITECTURE.md` §9 rule 5: "the filter runs before anything reaches
//! disk or the UI" — used both by the diagnostics bundle (`core::toolchain::diagnostics`)
//! and, later, the log sink (M9).

use regex::Regex;
use std::sync::LazyLock;

const PLACEHOLDER: &str = "«redacted»";

static ANTHROPIC_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"sk-ant-[A-Za-z0-9_-]+").unwrap());
static API_KEY_VAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)ANTHROPIC_API_KEY\s*=\s*\S+").unwrap());
static BEARER_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)bearer\s+[A-Za-z0-9._-]+").unwrap());
static SERIAL_VALUE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"SER=\S+").unwrap());

/// Redacts API keys, `ANTHROPIC_API_KEY=...` assignments, and bearer tokens. Serial numbers
/// (`SER=...`, from `pio device list` `hwid` strings) are only redacted when `redact_serials`
/// is set — they're useful for support debugging device-stickiness issues, so the app asks
/// before stripping them (`TOOLCHAIN-SETUP.md` §10: "if the user opts in").
pub fn redact(text: &str, redact_serials: bool) -> String {
    let mut out = ANTHROPIC_KEY.replace_all(text, PLACEHOLDER).into_owned();
    out = API_KEY_VAR
        .replace_all(&out, format!("ANTHROPIC_API_KEY={PLACEHOLDER}"))
        .into_owned();
    out = BEARER_TOKEN.replace_all(&out, format!("Bearer {PLACEHOLDER}")).into_owned();
    if redact_serials {
        out = SERIAL_VALUE.replace_all(&out, format!("SER={PLACEHOLDER}")).into_owned();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_anthropic_api_key() {
        let out = redact("here is sk-ant-abc123XYZ_-9 in a log line", false);
        assert!(!out.contains("abc123XYZ"));
        assert!(out.contains(PLACEHOLDER));
    }

    #[test]
    fn redacts_env_var_assignment() {
        let out = redact("ANTHROPIC_API_KEY=sk-ant-secretvalue", false);
        assert!(!out.contains("secretvalue"));
    }

    #[test]
    fn redacts_bearer_tokens() {
        let out = redact("Authorization: Bearer abc.def-ghi_123", false);
        assert!(!out.contains("abc.def-ghi_123"));
    }

    #[test]
    fn leaves_serial_numbers_alone_by_default() {
        let out = redact("hwid: USB VID:PID=10C4:EA60 SER=0001", false);
        assert!(out.contains("SER=0001"));
    }

    #[test]
    fn redacts_serial_numbers_when_opted_in() {
        let out = redact("hwid: USB VID:PID=10C4:EA60 SER=0001", true);
        assert!(!out.contains("SER=0001"));
    }

    #[test]
    fn leaves_ordinary_text_untouched() {
        let text = "Building .pio/build/esp32dev/firmware.bin";
        assert_eq!(redact(text, true), text);
    }
}
