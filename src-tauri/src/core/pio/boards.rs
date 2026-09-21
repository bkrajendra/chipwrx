//! Board catalogue: `pio boards --json-output` (the registry) with an offline fallback to
//! `pio boards --installed --json-output`. Cached to disk with a TTL. See `CLI-CONTRACT.md`
//! §3.1 and `DATA-MODEL.md` §9.

use crate::core::proc::{ProcKind, ProcessSupervisor, SpawnSpec};
use crate::core::toolchain::resolve::Resolution;
use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BoardBrief {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub mcu: String,
    pub fcpu: u64,
    pub ram: u64,
    pub rom: u64,
    pub frameworks: Vec<String>,
    pub vendor: String,
    pub url: String,
    pub connectivity: Vec<String>,
    pub debug_tools: Vec<String>,
}

#[derive(Deserialize)]
struct RawDebug {
    #[serde(default)]
    tools: BTreeMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct RawBoard {
    id: String,
    name: String,
    platform: String,
    mcu: String,
    fcpu: u64,
    ram: u64,
    rom: u64,
    frameworks: Vec<String>,
    vendor: String,
    url: String,
    #[serde(default)]
    connectivity: Vec<String>,
    #[serde(default)]
    debug: Option<RawDebug>,
}

impl From<RawBoard> for BoardBrief {
    fn from(r: RawBoard) -> Self {
        BoardBrief {
            id: r.id,
            name: r.name,
            platform: r.platform,
            mcu: r.mcu,
            fcpu: r.fcpu,
            ram: r.ram,
            rom: r.rom,
            frameworks: r.frameworks,
            vendor: r.vendor,
            url: r.url,
            connectivity: r.connectivity,
            debug_tools: r.debug.map(|d| d.tools.into_keys().collect()).unwrap_or_default(),
        }
    }
}

/// Parses `pio boards --json-output`'s array — tolerant of unknown extra fields, but a
/// single malformed entry fails the whole parse (matches `pio`'s own atomic-array output;
/// there's no meaningful "skip one board" recovery).
pub fn parse_boards_json(text: &str) -> Result<Vec<BoardBrief>> {
    let raw: Vec<RawBoard> = serde_json::from_str(text).map_err(|e| AppError::Io {
        message: format!("parsing `pio boards --json-output`: {e}"),
    })?;
    Ok(raw.into_iter().map(BoardBrief::from).collect())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BoardSource {
    Registry,
    Installed,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BoardCatalogue {
    pub schema_version: u32,
    /// RFC3339.
    pub fetched_at: String,
    pub pio_version: String,
    pub source: BoardSource,
    pub boards: Vec<BoardBrief>,
}

const CURRENT_SCHEMA_VERSION: u32 = 1;
const CACHE_FILE_NAME: &str = "boards.json";

fn cache_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(CACHE_FILE_NAME)
}

pub fn load_cache(cache_dir: &Path) -> Option<BoardCatalogue> {
    let bytes = std::fs::read(cache_path(cache_dir)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn save_cache(cache_dir: &Path, catalogue: &BoardCatalogue) -> Result<()> {
    std::fs::create_dir_all(cache_dir)?;
    let path = cache_path(cache_dir);
    let tmp = cache_dir.join(format!("{CACHE_FILE_NAME}.tmp-{}", std::process::id()));
    let json = serde_json::to_vec_pretty(catalogue)?;
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&json)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn is_stale(catalogue: &BoardCatalogue, ttl: Duration, now: chrono::DateTime<chrono::Utc>) -> bool {
    match chrono::DateTime::parse_from_rfc3339(&catalogue.fetched_at) {
        Ok(fetched) => {
            let age = now.signed_duration_since(fetched);
            age.to_std().map(|a| a >= ttl).unwrap_or(true)
        }
        Err(_) => true,
    }
}

async fn run_pio_boards(
    supervisor: &ProcessSupervisor,
    pio: &Resolution,
    cwd: &Path,
    installed_only: bool,
) -> Result<Vec<BoardBrief>> {
    let mut args = pio.extra_args.clone();
    args.push("boards".into());
    if installed_only {
        args.push("--installed".into());
    }
    args.push("--json-output".into());

    let spec = SpawnSpec {
        program: pio.program.clone(),
        args,
        cwd: cwd.to_path_buf(),
        env: vec![],
        kind: ProcKind::Pio,
        label: "pio-boards".into(),
    };
    let out = supervisor.spawn_capture(spec).await?;
    if out.exit_code != 0 {
        return Err(AppError::PioCommandFailed {
            argv: vec![pio.program.display().to_string(), "boards".into()],
            exit_code: out.exit_code,
            tail: tail(&format!("{}{}", out.stdout, out.stderr), 4096),
        });
    }
    parse_boards_json(&out.stdout)
}

fn tail(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        format!("…{}", &s[s.len() - max_bytes..])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogueStatus {
    /// Fetched fresh from the registry this call.
    FreshFromRegistry,
    /// Served from an unexpired disk cache.
    FreshFromCache,
    /// The registry fetch failed; served a stale disk cache instead.
    StaleOffline,
    /// No cache existed and the registry was unreachable; fell back to installed platforms.
    InstalledOffline,
    /// The caller explicitly asked for installed-only.
    InstalledOnly,
}

/// Orchestrates the full policy from `DATA-MODEL.md` §9: fresh cache is reused unless
/// `refresh`; otherwise try the registry; on failure fall back to a stale cache, and
/// failing that, to `--installed`.
pub async fn get_boards(
    supervisor: &ProcessSupervisor,
    pio: Option<&Resolution>,
    cwd: &Path,
    cache_dir: &Path,
    ttl: Duration,
    refresh: bool,
    installed_only: bool,
) -> Result<(Vec<BoardBrief>, CatalogueStatus)> {
    let Some(pio) = pio else {
        return Err(AppError::ToolMissing {
            tool: "pio".into(),
            install_action: true,
        });
    };

    let cached = load_cache(cache_dir);

    if installed_only {
        let boards = run_pio_boards(supervisor, pio, cwd, true).await?;
        return Ok((boards, CatalogueStatus::InstalledOnly));
    }

    if !refresh {
        if let Some(c) = &cached {
            if !is_stale(c, ttl, chrono::Utc::now()) {
                return Ok((c.boards.clone(), CatalogueStatus::FreshFromCache));
            }
        }
    }

    match run_pio_boards(supervisor, pio, cwd, false).await {
        Ok(boards) => {
            let catalogue = BoardCatalogue {
                schema_version: CURRENT_SCHEMA_VERSION,
                fetched_at: chrono::Utc::now().to_rfc3339(),
                pio_version: String::new(),
                source: BoardSource::Registry,
                boards: boards.clone(),
            };
            let _ = save_cache(cache_dir, &catalogue); // best-effort; serving fresh data either way
            Ok((boards, CatalogueStatus::FreshFromRegistry))
        }
        Err(_) if cached.is_some() => Ok((cached.unwrap().boards, CatalogueStatus::StaleOffline)),
        Err(_) => {
            let boards = run_pio_boards(supervisor, pio, cwd, true).await?;
            let catalogue = BoardCatalogue {
                schema_version: CURRENT_SCHEMA_VERSION,
                fetched_at: chrono::Utc::now().to_rfc3339(),
                pio_version: String::new(),
                source: BoardSource::Installed,
                boards: boards.clone(),
            };
            let _ = save_cache(cache_dir, &catalogue);
            Ok((boards, CatalogueStatus::InstalledOffline))
        }
    }
}

/// Looks up one board by id — checks the disk cache first (any staleness; a board's specs
/// don't change), then falls back to a targeted `pio boards <id> --json-output` rather than
/// fetching the full ~1500-entry catalogue just to regenerate one `CLAUDE.md`.
pub async fn find_board(
    supervisor: &ProcessSupervisor,
    pio: &Resolution,
    cwd: &Path,
    cache_dir: &Path,
    board_id: &str,
) -> Result<BoardBrief> {
    if let Some(cached) = load_cache(cache_dir) {
        if let Some(b) = cached.boards.into_iter().find(|b| b.id == board_id) {
            return Ok(b);
        }
    }

    let mut args = pio.extra_args.clone();
    args.push("boards".into());
    args.push(board_id.to_string());
    args.push("--json-output".into());
    let spec = SpawnSpec {
        program: pio.program.clone(),
        args,
        cwd: cwd.to_path_buf(),
        env: vec![],
        kind: ProcKind::Pio,
        label: "pio-boards-lookup".into(),
    };
    let out = supervisor.spawn_capture(spec).await?;
    if out.exit_code != 0 {
        return Err(AppError::PioCommandFailed {
            argv: vec![pio.program.display().to_string(), "boards".into(), board_id.into()],
            exit_code: out.exit_code,
            tail: tail(&format!("{}{}", out.stdout, out.stderr), 4096),
        });
    }
    let boards = parse_boards_json(&out.stdout)?;
    boards
        .into_iter()
        .find(|b| b.id == board_id)
        .ok_or_else(|| AppError::Io {
            message: format!("board '{board_id}' not found"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[
      {
        "id": "esp32dev",
        "name": "Espressif ESP32 Dev Module",
        "platform": "espressif32",
        "mcu": "ESP32",
        "fcpu": 240000000,
        "ram": 327680,
        "rom": 4194304,
        "frameworks": ["arduino", "espidf"],
        "vendor": "Espressif",
        "url": "https://example.com",
        "connectivity": ["wifi", "bluetooth"],
        "debug": { "tools": { "esp-prog": { "default": true }, "jlink": {} } }
      },
      {
        "id": "uno",
        "name": "Arduino Uno",
        "platform": "atmelavr",
        "mcu": "ATMEGA328P",
        "fcpu": 16000000,
        "ram": 2048,
        "rom": 32256,
        "frameworks": ["arduino"],
        "vendor": "Arduino",
        "url": "https://example.com"
      }
    ]"#;

    #[test]
    fn parses_boards_with_and_without_optional_fields() {
        let boards = parse_boards_json(SAMPLE).expect("parse");
        assert_eq!(boards.len(), 2);

        let esp32 = &boards[0];
        assert_eq!(esp32.id, "esp32dev");
        assert_eq!(esp32.connectivity, vec!["wifi", "bluetooth"]);
        let mut tools = esp32.debug_tools.clone();
        tools.sort();
        assert_eq!(tools, vec!["esp-prog", "jlink"]);

        let uno = &boards[1];
        assert!(uno.connectivity.is_empty());
        assert!(uno.debug_tools.is_empty());
    }

    #[test]
    fn cache_round_trips_through_save_and_load() {
        let dir = std::env::temp_dir().join(format!("vibe-hw-boards-test-{}", uuid::Uuid::new_v4()));
        let catalogue = BoardCatalogue {
            schema_version: CURRENT_SCHEMA_VERSION,
            fetched_at: chrono::Utc::now().to_rfc3339(),
            pio_version: "6.2.0".into(),
            source: BoardSource::Registry,
            boards: parse_boards_json(SAMPLE).unwrap(),
        };
        save_cache(&dir, &catalogue).expect("save");
        let loaded = load_cache(&dir).expect("load");
        assert_eq!(loaded, catalogue);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_cache_returns_none() {
        let dir = std::env::temp_dir().join(format!("vibe-hw-boards-missing-{}", uuid::Uuid::new_v4()));
        assert!(load_cache(&dir).is_none());
    }

    #[test]
    fn freshness_is_ttl_bounded() {
        let now = chrono::Utc::now();
        let catalogue = BoardCatalogue {
            schema_version: CURRENT_SCHEMA_VERSION,
            fetched_at: (now - chrono::Duration::hours(10)).to_rfc3339(),
            pio_version: "6.2.0".into(),
            source: BoardSource::Registry,
            boards: vec![],
        };
        assert!(!is_stale(&catalogue, Duration::from_secs(24 * 3600), now));
        assert!(is_stale(&catalogue, Duration::from_secs(5 * 3600), now));
    }

    #[test]
    fn unparseable_fetched_at_is_treated_as_stale() {
        let catalogue = BoardCatalogue {
            schema_version: CURRENT_SCHEMA_VERSION,
            fetched_at: "not a date".into(),
            pio_version: "6.2.0".into(),
            source: BoardSource::Registry,
            boards: vec![],
        };
        assert!(is_stale(&catalogue, Duration::from_secs(999_999), chrono::Utc::now()));
    }
}
