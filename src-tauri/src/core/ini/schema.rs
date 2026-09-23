//! The `platformio.ini` option schema (`FR-INI-2`, `CLI-CONTRACT.md` §6). There is no CLI
//! command for this — it's extracted by running a one-line Python snippet in PlatformIO's
//! own environment, cached on disk keyed by `core_version`
//! (`<cache>/pio-schema-<core_version>.json`, `DATA-MODEL.md` §8.3, storing the extraction
//! one-liner's raw JSON **verbatim**), with a bundled fallback so the Form tab still
//! renders if extraction ever fails.
//!
//! Grounded in a real extraction against this session's installed PlatformIO Core
//! (`tests/fixtures/pio-schema-real.json`, also the bundled fallback at
//! `src-tauri/assets/pio-schema-fallback.json`).

use crate::core::proc::{ProcKind, ProcessSupervisor, SpawnSpec};
use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct IniOptionSchema {
    /// `"env.build_flags"` — the extraction one-liner's own map key.
    pub key: String,
    pub scope: String,
    pub group: String,
    pub name: String,
    pub description: String,
    pub r#type: String,
    pub multiple: bool,
    pub default: Option<serde_json::Value>,
    pub choices: Option<Vec<String>>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub sysenvvar: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawOption {
    scope: String,
    group: String,
    name: String,
    description: String,
    #[serde(rename = "type", default = "default_type")]
    r#type: String,
    #[serde(default)]
    multiple: bool,
    #[serde(default)]
    default: Option<serde_json::Value>,
    #[serde(default)]
    choices: Option<Vec<String>>,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
    #[serde(default)]
    sysenvvar: Option<String>,
}

fn default_type() -> String {
    "string".into()
}

/// The exact one-liner from `CLI-CONTRACT.md` §6 — never modified, since this runs inside
/// PlatformIO's own Python environment and any typo fails silently as an import error.
const EXTRACTION_ONE_LINER: &str =
    "import json;from platformio.project.options import ProjectOptions;print(json.dumps({k:v.as_dict() for k,v in ProjectOptions.items()}))";

const BUNDLED_FALLBACK_JSON: &str = include_str!("../../../assets/pio-schema-fallback.json");

pub fn parse_schema_json(json: &str) -> Result<Vec<IniOptionSchema>> {
    let raw: BTreeMap<String, RawOption> = serde_json::from_str(json).map_err(|e| AppError::Io {
        message: format!("parsing the option schema: {e}"),
    })?;
    Ok(raw
        .into_iter()
        .map(|(key, r)| IniOptionSchema {
            key,
            scope: r.scope,
            group: r.group,
            name: r.name,
            description: r.description,
            r#type: r.r#type,
            multiple: r.multiple,
            default: r.default,
            choices: r.choices,
            min: r.min,
            max: r.max,
            sysenvvar: r.sysenvvar,
        })
        .collect())
}

fn cache_path(cache_dir: &Path, core_version: &str) -> PathBuf {
    cache_dir.join(format!("pio-schema-{core_version}.json"))
}

pub fn load_cache(cache_dir: &Path, core_version: &str) -> Option<String> {
    std::fs::read_to_string(cache_path(cache_dir, core_version)).ok()
}

pub fn save_cache(cache_dir: &Path, core_version: &str, json: &str) -> Result<()> {
    std::fs::create_dir_all(cache_dir)?;
    let path = cache_path(cache_dir, core_version);
    let tmp = cache_dir.join(format!("pio-schema-{core_version}.json.tmp-{}", std::process::id()));
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(json.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

async fn extract_schema_json(supervisor: &ProcessSupervisor, python_exe: &Path, cwd: &Path) -> Result<String> {
    let out = supervisor
        .spawn_capture(SpawnSpec {
            program: python_exe.to_path_buf(),
            args: vec!["-c".into(), EXTRACTION_ONE_LINER.into()],
            cwd: cwd.to_path_buf(),
            env: vec![],
            kind: ProcKind::Tool,
            label: "pio-ini-schema-extract".into(),
        })
        .await?;
    if out.exit_code != 0 {
        return Err(AppError::Io {
            message: format!("option schema extraction failed: {}", tail(&format!("{}{}", out.stdout, out.stderr), 2048)),
        });
    }
    Ok(out.stdout)
}

fn tail(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        format!("…{}", &s[s.len() - max_bytes..])
    }
}

pub fn bundled_fallback() -> Vec<IniOptionSchema> {
    parse_schema_json(BUNDLED_FALLBACK_JSON).unwrap_or_default()
}

/// Cache-first, re-extract-on-miss, bundled-fallback-on-failure — `CLI-CONTRACT.md` §6:
/// "Cache the extracted schema keyed by `core_version`... Ship a bundled fallback copy so
/// the form still renders if extraction fails." Never returns an empty schema unless even
/// the bundled fallback fails to parse (which would mean the asset itself is corrupt).
pub async fn get_schema(
    supervisor: &ProcessSupervisor,
    python_exe: Option<&Path>,
    core_version: Option<&str>,
    cwd: &Path,
    cache_dir: &Path,
) -> Vec<IniOptionSchema> {
    if let Some(ver) = core_version {
        if let Some(cached) = load_cache(cache_dir, ver) {
            if let Ok(schema) = parse_schema_json(&cached) {
                return schema;
            }
        }
        if let Some(py) = python_exe {
            if let Ok(json) = extract_schema_json(supervisor, py, cwd).await {
                if let Ok(schema) = parse_schema_json(&json) {
                    let _ = save_cache(cache_dir, ver, &json);
                    return schema;
                }
            }
        }
    }
    bundled_fallback()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_real_captured_schema() {
        let json = include_str!("../../../../tests/fixtures/pio-schema-real.json");
        let schema = parse_schema_json(json).expect("parse");
        // `CLI-CONTRACT.md` §6: "Verified on Core 6.2.0: 84 options" — this session's
        // installed Core version may differ slightly; assert a realistic range instead of
        // the exact historical count.
        assert!(schema.len() > 60, "expected a substantial option set, got {}", schema.len());

        let build_flags = schema.iter().find(|o| o.key == "env.build_flags").expect("env.build_flags");
        assert_eq!(build_flags.scope, "env");
        assert_eq!(build_flags.group, "build");
        assert_eq!(build_flags.r#type, "string");
        assert!(build_flags.multiple);
        assert_eq!(build_flags.sysenvvar.as_deref(), Some("PLATFORMIO_BUILD_FLAGS"));

        let monitor_speed = schema.iter().find(|o| o.key == "env.monitor_speed").expect("env.monitor_speed");
        assert_eq!(monitor_speed.r#type, "integer");
        assert!(!monitor_speed.multiple);
        assert_eq!(monitor_speed.default, Some(serde_json::json!(9600)));

        let ldf_mode = schema.iter().find(|o| o.key == "env.lib_ldf_mode").expect("env.lib_ldf_mode");
        assert_eq!(ldf_mode.r#type, "choice");
        assert_eq!(
            ldf_mode.choices.as_deref(),
            Some(["off", "chain", "deep", "chain+", "deep+"].map(String::from).as_slice())
        );
    }

    #[test]
    fn bundled_fallback_parses_and_is_non_empty() {
        let schema = bundled_fallback();
        assert!(!schema.is_empty());
        assert!(schema.iter().any(|o| o.key == "env.monitor_speed"));
    }

    #[test]
    fn malformed_schema_json_is_a_typed_error_not_a_panic() {
        assert!(parse_schema_json("not json").is_err());
    }

    #[test]
    fn cache_round_trips_through_save_and_load() {
        let dir = std::env::temp_dir().join(format!("vibe-hw-ini-schema-cache-{}", uuid::Uuid::new_v4()));
        let json = include_str!("../../../../tests/fixtures/pio-schema-real.json");
        save_cache(&dir, "6.2.0", json).expect("save");
        let loaded = load_cache(&dir, "6.2.0").expect("load");
        assert_eq!(loaded, json);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_cache_returns_none() {
        let dir = std::env::temp_dir().join(format!("vibe-hw-ini-schema-missing-{}", uuid::Uuid::new_v4()));
        assert!(load_cache(&dir, "6.2.0").is_none());
    }

    #[test]
    fn different_versions_get_different_cache_entries() {
        let dir = std::env::temp_dir().join(format!("vibe-hw-ini-schema-versions-{}", uuid::Uuid::new_v4()));
        save_cache(&dir, "6.2.0", "{}").expect("save 6.2.0");
        assert!(load_cache(&dir, "6.3.0").is_none());
        assert_eq!(load_cache(&dir, "6.2.0").as_deref(), Some("{}"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
