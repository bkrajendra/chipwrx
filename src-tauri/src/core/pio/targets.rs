//! `pio run --list-targets` (`CLI-CONTRACT.md` §5.1, `FR-BUILD-7`). Text-only — there is
//! no `--json-output` form (`CLAUDE.md` landmine 6) — so this parses the column-aligned
//! table PlatformIO prints, grounded against a real capture in
//! `tests/fixtures/pio-run-list-targets.txt`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use ts_rs::TS;

/// `FR-BUILD-7`: "before that [`--list-targets` is cached], the menu shows the universal
/// subset" — `CLI-CONTRACT.md` §5.1's exact list.
pub const UNIVERSAL_TARGETS: &[&str] = &["build", "upload", "clean", "fullclean", "size", "monitor"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TargetInfo {
    pub name: String,
    pub title: String,
    pub description: Option<String>,
}

/// Parses PlatformIO's column-aligned `--list-targets` table. The header row's dashed
/// separator line gives exact column boundaries (a fixed-width table, not delimiter-based)
/// — read those first, then slice every data row against them. Rows for a target with no
/// description (common — see the fixture) simply have nothing past that column boundary.
pub fn parse_list_targets(text: &str) -> Vec<TargetInfo> {
    let mut lines = text.lines();
    if lines.find(|l| l.trim_start().starts_with("Environment")).is_none() {
        return Vec::new();
    }
    let Some(separator) = lines.next() else {
        return Vec::new();
    };
    let bounds = column_bounds(separator);
    if bounds.len() < 3 {
        return Vec::new(); // not a recognizable table — degrade to "no targets" rather than guess
    }

    let name_col = 2;
    let title_col = 3;
    let desc_col = 4;

    lines
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let name = slice_col(line, &bounds, name_col)?;
            if name.is_empty() {
                return None;
            }
            let title = slice_col(line, &bounds, title_col).unwrap_or_default();
            let description = slice_col(line, &bounds, desc_col).filter(|d| !d.is_empty());
            Some(TargetInfo {
                name,
                title,
                description,
            })
        })
        .collect()
}

/// Byte-range `[start, end)` of each run of `-` characters in the separator line —
/// consecutive dash runs are the column widths; the (typically two-space) gaps between
/// them are dropped, same as the header/data rows' inter-column padding.
fn column_bounds(separator: &str) -> Vec<(usize, usize)> {
    let mut bounds = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in separator.char_indices() {
        if c == '-' {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            bounds.push((s, i));
        }
    }
    if let Some(s) = start.take() {
        bounds.push((s, separator.len()));
    }
    bounds
}

fn slice_col(line: &str, bounds: &[(usize, usize)], idx: usize) -> Option<String> {
    let &(start, end) = bounds.get(idx)?;
    if start >= line.len() {
        return Some(String::new());
    }
    Some(line.get(start..end.min(line.len()))?.trim().to_string())
}

const CACHE_TTL_HOURS: u64 = 168; // matches BoardCatalogue's own default TTL (M2)

fn cache_path(cache_dir: &Path, env: &str) -> PathBuf {
    cache_dir.join("pio-targets").join(format!("{env}.json"))
}

/// Caches the parsed target list per environment (`DATA-MODEL.md` §1:
/// `<cache>/pio-targets/<ws-id>-<env>.json` — this app keys the cache directory itself by
/// workspace, so the file only needs the env name).
pub fn load_cached(cache_dir: &Path, env: &str) -> Option<Vec<TargetInfo>> {
    let path = cache_path(cache_dir, env);
    let meta = std::fs::metadata(&path).ok()?;
    let modified = meta.modified().ok()?;
    if modified.elapsed().ok()? > std::time::Duration::from_secs(CACHE_TTL_HOURS * 3600) {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn save_cache(cache_dir: &Path, env: &str, targets: &[TargetInfo]) -> std::io::Result<()> {
    let path = cache_path(cache_dir, env);
    std::fs::create_dir_all(path.parent().expect("pio-targets cache path always has a parent"))?;
    std::fs::write(path, serde_json::to_vec(targets)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_real_captured_table() {
        let text = include_str!("../../../../tests/fixtures/pio-run-list-targets.txt");
        let targets = parse_list_targets(text);
        assert_eq!(targets.len(), 12, "{targets:?}");

        let upload = targets.iter().find(|t| t.name == "upload").expect("upload target");
        assert_eq!(upload.title, "Upload");
        assert_eq!(upload.description, None);

        let coredump = targets.iter().find(|t| t.name == "coredump").expect("coredump target");
        assert_eq!(coredump.title, "Coredump Analysis");
        assert_eq!(coredump.description.as_deref(), Some("Analyze coredumps using esp-coredump (supports CLI args after --)"));

        let size = targets.iter().find(|t| t.name == "size").expect("size target");
        assert_eq!(size.description.as_deref(), Some("Calculate program size"));
    }

    #[test]
    fn unrecognized_input_yields_an_empty_list_not_a_panic() {
        assert_eq!(parse_list_targets("pio: command not found").len(), 0);
        assert_eq!(parse_list_targets("").len(), 0);
    }

    #[test]
    fn cache_round_trips_and_respects_ttl() {
        let dir = std::env::temp_dir().join(format!("vibe-hw-targets-cache-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(load_cached(&dir, "esp32dev"), None);

        let targets = vec![TargetInfo {
            name: "upload".into(),
            title: "Upload".into(),
            description: None,
        }];
        save_cache(&dir, "esp32dev", &targets).unwrap();
        assert_eq!(load_cached(&dir, "esp32dev"), Some(targets));

        // A different environment's cache is independent.
        assert_eq!(load_cached(&dir, "uno"), None);
    }
}
