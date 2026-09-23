//! Just enough of `platformio.ini` to populate the environment picker (`FR-PROJ-7`): the
//! names of `[env:*]` sections. **Not** the format-preserving editor — that's `core::ini`,
//! scoped to M7. This never writes the file; the active env is tracked in
//! `.vibe/project.json`, not by rewriting `platformio.ini`'s `default_envs`.

/// Extracts `[env:xxx]` section names in file order, de-duplicated. Tolerant of comments,
/// blank lines, and any other section shape — a malformed line is just not a match.
pub fn list_env_names(ini_text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in ini_text.lines() {
        let trimmed = line.trim();
        let Some(inner) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) else {
            continue;
        };
        let Some(name) = inner.strip_prefix("env:") else {
            continue;
        };
        let name = name.trim();
        if !name.is_empty() && !names.iter().any(|n: &String| n == name) {
            names.push(name.to_string());
        }
    }
    names
}

/// Reads a single `key = value` out of one `[section]` (exact name match, e.g. `"env:esp32dev"`
/// or `"platformio"`). Single-valued only — good enough for `board`/`framework`/`default_envs`
/// lookups; a `core::ini`-grade parser (continuation lines, multi-value options, comments as
/// first-class data) is M7's job, not this scanner's.
pub fn read_value(ini_text: &str, section: &str, key: &str) -> Option<String> {
    let mut in_section = false;
    for line in ini_text.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_section = name.trim() == section;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            if k.trim() == key {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// Reads `key` from `[env:<env_name>]`, falling back to the generic `[env]` section —
/// matches PlatformIO's own inheritance for env-scoped options like `monitor_*`
/// (`CLI-CONTRACT.md` §3.3: "it reads the env's `monitor_*` options", and the worked
/// example at §2.4 shows `monitor_speed` set in `[env]` and inherited into `[env:esp32dev]`).
pub fn read_value_with_fallback(ini_text: &str, env_name: &str, key: &str) -> Option<String> {
    read_value(ini_text, &format!("env:{env_name}"), key).or_else(|| read_value(ini_text, "env", key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_env_sections_among_others() {
        let ini = "\
[platformio]
default_envs = esp32dev

[env]
monitor_speed = 115200

[env:esp32dev]
platform = espressif32
board = esp32dev
framework = arduino

[env:esp32dev-ota]
platform = espressif32
board = esp32dev
framework = arduino
upload_protocol = espota
";
        assert_eq!(list_env_names(ini), vec!["esp32dev", "esp32dev-ota"]);
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let ini = "\
; a comment mentioning [env:decoy] should not count
# another style of comment [env:also-decoy]

[env:real]
platform = espressif32
";
        assert_eq!(list_env_names(ini), vec!["real"]);
    }

    #[test]
    fn no_env_sections_returns_empty() {
        let ini = "[platformio]\ndefault_envs = esp32dev\n";
        assert!(list_env_names(ini).is_empty());
    }

    #[test]
    fn deduplicates_a_redeclared_section() {
        let ini = "[env:esp32dev]\nplatform = espressif32\n\n[env:esp32dev]\nboard = esp32dev\n";
        assert_eq!(list_env_names(ini), vec!["esp32dev"]);
    }

    #[test]
    fn tolerates_indentation_around_the_brackets() {
        let ini = "   [env:indented]   \nplatform = espressif32\n";
        assert_eq!(list_env_names(ini), vec!["indented"]);
    }

    const MULTI_ENV_INI: &str = "\
[platformio]
default_envs = esp32dev

[env:esp32dev]
platform = espressif32
board = esp32dev
framework = arduino

[env:uno]
platform = atmelavr
board = uno
framework = arduino
";

    #[test]
    fn read_value_finds_a_key_in_the_named_section() {
        assert_eq!(
            read_value(MULTI_ENV_INI, "env:esp32dev", "board"),
            Some("esp32dev".to_string())
        );
        assert_eq!(read_value(MULTI_ENV_INI, "env:uno", "board"), Some("uno".to_string()));
        assert_eq!(
            read_value(MULTI_ENV_INI, "platformio", "default_envs"),
            Some("esp32dev".to_string())
        );
    }

    #[test]
    fn read_value_does_not_leak_across_sections() {
        // "framework" exists under both envs but with different values — must not return
        // esp32dev's value when asked about a key that (hypothetically) only uno had.
        assert_eq!(read_value(MULTI_ENV_INI, "env:uno", "framework"), Some("arduino".to_string()));
        assert_eq!(read_value(MULTI_ENV_INI, "env:does-not-exist", "board"), None);
    }

    #[test]
    fn read_value_missing_key_returns_none() {
        assert_eq!(read_value(MULTI_ENV_INI, "env:esp32dev", "upload_protocol"), None);
    }

    const INI_WITH_INHERITED_MONITOR: &str = "\
[env]
monitor_speed = 115200

[env:esp32dev]
platform = espressif32
board = esp32dev

[env:uno]
platform = atmelavr
board = uno
monitor_speed = 9600
";

    #[test]
    fn fallback_inherits_from_the_generic_env_section() {
        assert_eq!(
            read_value_with_fallback(INI_WITH_INHERITED_MONITOR, "esp32dev", "monitor_speed"),
            Some("115200".to_string())
        );
    }

    #[test]
    fn fallback_prefers_the_named_envs_own_value() {
        assert_eq!(
            read_value_with_fallback(INI_WITH_INHERITED_MONITOR, "uno", "monitor_speed"),
            Some("9600".to_string())
        );
    }

    #[test]
    fn fallback_returns_none_when_neither_section_has_the_key() {
        assert_eq!(read_value_with_fallback(INI_WITH_INHERITED_MONITOR, "esp32dev", "upload_protocol"), None);
    }
}
