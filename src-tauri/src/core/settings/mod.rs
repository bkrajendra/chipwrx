//! `settings.json` — `GlobalSettings`. See `DATA-MODEL.md` §3.
//!
//! Deliberately Tauri-independent: callers resolve the app config directory via Tauri's
//! path API and pass it in, so this stays unit-testable with a plain temp directory.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use ts_rs::TS;

const CURRENT_SCHEMA_VERSION: u32 = 1;
const FILE_NAME: &str = "settings.json";

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolchainSettings {
    /// `null` = auto-resolve (`core/toolchain::resolve`).
    pub claude_path: Option<String>,
    pub pio_path: Option<String>,
    pub python_path: Option<String>,
    pub auto_probe_on_focus: bool,
}

impl Default for ToolchainSettings {
    fn default() -> Self {
        Self {
            claude_path: None,
            pio_path: None,
            python_path: None,
            auto_probe_on_focus: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PermissionPolicySetting {
    Guarded,
    Assisted,
    Unrestricted,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSettings {
    pub permission_policy: PermissionPolicySetting,
    pub model: String,
    pub max_turns: Option<u32>,
    pub show_thinking: bool,
    pub show_cost_estimate: bool,
}

impl Default for ClaudeSettings {
    fn default() -> Self {
        Self {
            permission_policy: PermissionPolicySetting::Guarded,
            model: "sonnet".into(),
            max_turns: None,
            show_thinking: false,
            show_cost_estimate: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PipelinePolicySetting {
    Safe,
    FastPath,
    Watch,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PipelineSettings {
    pub policy: PipelinePolicySetting,
    pub auto_reattach_monitor: bool,
    pub parallel_jobs: Option<u32>,
    pub stop_grace_ms: u32,
}

impl Default for PipelineSettings {
    fn default() -> Self {
        Self {
            policy: PipelinePolicySetting::Safe,
            auto_reattach_monitor: true,
            parallel_jobs: None,
            stop_grace_ms: 3000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MonitorSettings {
    pub max_lines: u32,
    pub max_bytes: u64,
    pub timestamps: bool,
    pub autoscroll: bool,
}

impl Default for MonitorSettings {
    fn default() -> Self {
        Self {
            max_lines: 20_000,
            max_bytes: 8_388_608,
            timestamps: false,
            autoscroll: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LogSettings {
    pub max_lines: u32,
}

impl Default for LogSettings {
    fn default() -> Self {
        Self { max_lines: 50_000 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EditorSettings {
    pub command: Option<String>,
    pub goto_line_arg_template: String,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            command: None,
            goto_line_arg_template: "--goto {file}:{line}".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ThemeSetting {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceSettings {
    pub theme: ThemeSetting,
    pub font_scale: f32,
    pub mono_font: Option<String>,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: ThemeSetting::System,
            font_scale: 1.0,
            mono_font: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSettings {
    pub registry_timeout_ms: u32,
    pub board_catalogue_ttl_hours: u32,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            registry_timeout_ms: 20_000,
            board_catalogue_ttl_hours: 168,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AdvancedSettings {
    pub keep_process_logs: bool,
    /// Reset to `false` on every app version change (`DATA-MODEL.md` §3 migration rule).
    pub allow_unrestricted_policy: bool,
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self {
            keep_process_logs: true,
            allow_unrestricted_policy: false,
        }
    }
}

/// `NFR-D2`: "Tauri updater with a signed update manifest; update checks are opt-out" —
/// on by default, one toggle to turn off. Manually clicking "Check for updates" in Global
/// Settings always works regardless of this flag; it only gates the automatic
/// startup/background check.
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettings {
    pub check_for_updates: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self { check_for_updates: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSettings {
    pub schema_version: u32,
    /// The app version that last wrote this file — compared at startup against the
    /// running app's own version to drive the `apply_version_migration` reset. `None`
    /// covers both a fresh install and a file written before this field existed; either
    /// way, resetting is the safe default (`DATA-MODEL.md` §3).
    #[serde(default)]
    pub last_app_version: Option<String>,
    #[serde(default)]
    pub toolchain: ToolchainSettings,
    #[serde(default)]
    pub claude: ClaudeSettings,
    #[serde(default)]
    pub pipeline: PipelineSettings,
    #[serde(default)]
    pub monitor: MonitorSettings,
    #[serde(default)]
    pub logs: LogSettings,
    #[serde(default)]
    pub editor: EditorSettings,
    #[serde(default)]
    pub appearance: AppearanceSettings,
    #[serde(default)]
    pub network: NetworkSettings,
    #[serde(default)]
    pub advanced: AdvancedSettings,
    #[serde(default)]
    pub updates: UpdateSettings,
    /// `FR-SETUP-8`: "Completion state is per-machine, in `settings.json`, not
    /// per-project" — the 4-step first-run onboarding is skippable and re-enterable from
    /// Doctor, but only shows automatically once.
    #[serde(default)]
    pub onboarding_completed: bool,
    /// `NFR-S2`/`M10`: "A first-run notice states plainly that prompts, and the code
    /// Claude reads, are sent to Anthropic by the Claude CLI." Gates showing that notice —
    /// separate from `onboarding_completed` since it's a one-time disclosure, not a
    /// skippable-and-re-enterable wizard like onboarding is.
    #[serde(default)]
    pub privacy_notice_acknowledged: bool,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            last_app_version: None,
            toolchain: ToolchainSettings::default(),
            claude: ClaudeSettings::default(),
            pipeline: PipelineSettings::default(),
            monitor: MonitorSettings::default(),
            logs: LogSettings::default(),
            editor: EditorSettings::default(),
            appearance: AppearanceSettings::default(),
            network: NetworkSettings::default(),
            advanced: AdvancedSettings::default(),
            updates: UpdateSettings::default(),
            onboarding_completed: false,
            privacy_notice_acknowledged: false,
        }
    }
}

/// What happened while loading, so the caller can toast the user (`DATA-MODEL.md` §12).
#[derive(Debug, Clone, PartialEq)]
pub enum LoadOutcome {
    Loaded,
    Created,
    RecoveredFromCorruption { corrupt_backup: PathBuf },
    /// `DATA-MODEL.md` §12: "`schemaVersion` lower than current: migrate in memory,
    /// re-write atomically, log the migration" — `from_version` is what the file had before
    /// this load bumped it to `CURRENT_SCHEMA_VERSION` and re-saved. Migration itself is
    /// just the field-level `#[serde(default)]`s on every section already doing their job;
    /// this variant exists to mark that it happened and get logged (`M9`).
    Migrated { from_version: u32 },
    /// `DATA-MODEL.md` §12: "`schemaVersion` higher than current: load read-only; banner:
    /// 'This project was used with a newer version...'" — the file is deliberately **not**
    /// re-written in this case (a downgrade must never silently discard fields a newer
    /// version wrote that this version doesn't know about). `M9`'s `SPEC.md` §8 open
    /// question 42: the caller enforcing "read-only" (refusing `settings_set_global` for
    /// the rest of this session) isn't wired up yet — this only guarantees the file itself
    /// is left untouched.
    NewerSchema { found_version: u32 },
}

fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE_NAME)
}

/// Loads `settings.json`, creating it with defaults if absent, and recovering from a
/// corrupt file by renaming it aside (`<name>.corrupt-<unix-ts>`) rather than losing the
/// user's other config or refusing to start.
pub fn load(config_dir: &Path) -> std::io::Result<(GlobalSettings, LoadOutcome)> {
    let path = settings_path(config_dir);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let defaults = GlobalSettings::default();
            save(config_dir, &defaults)?;
            return Ok((defaults, LoadOutcome::Created));
        }
        Err(e) => return Err(e),
    };

    match serde_json::from_slice::<GlobalSettings>(&bytes) {
        Ok(mut settings) => {
            if settings.schema_version < CURRENT_SCHEMA_VERSION {
                let from_version = settings.schema_version;
                tracing::info!("migrating settings.json from schema v{from_version} to v{CURRENT_SCHEMA_VERSION}");
                settings.schema_version = CURRENT_SCHEMA_VERSION;
                save(config_dir, &settings)?;
                Ok((settings, LoadOutcome::Migrated { from_version }))
            } else if settings.schema_version > CURRENT_SCHEMA_VERSION {
                let found_version = settings.schema_version;
                tracing::warn!(
                    "settings.json is schema v{found_version} — newer than this app's v{CURRENT_SCHEMA_VERSION}; leaving the file untouched"
                );
                Ok((settings, LoadOutcome::NewerSchema { found_version }))
            } else {
                Ok((settings, LoadOutcome::Loaded))
            }
        }
        Err(_) => {
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let backup = config_dir.join(format!("{FILE_NAME}.corrupt-{ts}"));
            std::fs::rename(&path, &backup)?;
            let defaults = GlobalSettings::default();
            save(config_dir, &defaults)?;
            Ok((
                defaults,
                LoadOutcome::RecoveredFromCorruption {
                    corrupt_backup: backup,
                },
            ))
        }
    }
}

/// Atomic write: temp file in the same directory, `fsync`, rename (`NFR-R3`) — a crash
/// mid-write never leaves a truncated `settings.json`.
pub fn save(config_dir: &Path, settings: &GlobalSettings) -> std::io::Result<()> {
    std::fs::create_dir_all(config_dir)?;
    let path = settings_path(config_dir);
    let tmp_path = config_dir.join(format!("{FILE_NAME}.tmp-{}", std::process::id()));

    let json = serde_json::to_vec_pretty(settings)?;
    {
        let mut f = std::fs::File::create(&tmp_path)?;
        f.write_all(&json)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp_path, &path)?;
    Ok(())
}

/// `DATA-MODEL.md` §3: "`claude.permissionPolicy` is forced back to `guarded` and
/// `advanced.allowUnrestrictedPolicy` to `false` whenever the app version changes"
/// (`FR-CHAT-4`, `NFR-S3`) — Unrestricted is meant to be re-confirmed per install, not
/// silently carried across an update. Returns `true` if anything changed (so the caller
/// knows whether a re-save is needed), and always leaves `last_app_version` set to
/// `current_version`.
pub fn apply_version_migration(settings: &mut GlobalSettings, current_version: &str) -> bool {
    if settings.last_app_version.as_deref() == Some(current_version) {
        return false;
    }
    settings.last_app_version = Some(current_version.to_string());
    settings.claude.permission_policy = PermissionPolicySetting::Guarded;
    settings.advanced.allow_unrestricted_policy = false;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_migration_resets_unrestricted_policy_on_a_version_change() {
        let mut settings = GlobalSettings {
            last_app_version: Some("0.1.0".into()),
            ..Default::default()
        };
        settings.claude.permission_policy = PermissionPolicySetting::Unrestricted;
        settings.advanced.allow_unrestricted_policy = true;

        let changed = apply_version_migration(&mut settings, "0.2.0");
        assert!(changed);
        assert_eq!(settings.claude.permission_policy, PermissionPolicySetting::Guarded);
        assert!(!settings.advanced.allow_unrestricted_policy);
        assert_eq!(settings.last_app_version.as_deref(), Some("0.2.0"));
    }

    #[test]
    fn version_migration_is_a_no_op_on_the_same_version() {
        let mut settings = GlobalSettings {
            last_app_version: Some("0.1.0".into()),
            ..Default::default()
        };
        settings.claude.permission_policy = PermissionPolicySetting::Unrestricted;

        let changed = apply_version_migration(&mut settings, "0.1.0");
        assert!(!changed);
        assert_eq!(settings.claude.permission_policy, PermissionPolicySetting::Unrestricted);
    }

    #[test]
    fn version_migration_resets_on_first_run_with_no_recorded_version() {
        // Covers both a fresh install and a settings.json written before this field
        // existed — resetting is the safe default either way.
        let mut settings = GlobalSettings::default();
        settings.claude.permission_policy = PermissionPolicySetting::Unrestricted;
        assert_eq!(settings.last_app_version, None);

        let changed = apply_version_migration(&mut settings, "0.1.0");
        assert!(changed);
        assert_eq!(settings.claude.permission_policy, PermissionPolicySetting::Guarded);
        assert_eq!(settings.last_app_version.as_deref(), Some("0.1.0"));
    }

    #[test]
    fn missing_file_creates_defaults() {
        let dir = tempdir();
        let (settings, outcome) = load(&dir).expect("load");
        assert_eq!(outcome, LoadOutcome::Created);
        assert_eq!(settings, GlobalSettings::default());
        assert!(settings_path(&dir).exists());
    }

    #[test]
    fn round_trips_through_save_and_load() {
        let dir = tempdir();
        let mut settings = GlobalSettings::default();
        settings.toolchain.claude_path = Some("/usr/local/bin/claude".into());
        settings.claude.permission_policy = PermissionPolicySetting::Assisted;
        save(&dir, &settings).expect("save");

        let (loaded, outcome) = load(&dir).expect("load");
        assert_eq!(outcome, LoadOutcome::Loaded);
        assert_eq!(loaded, settings);
    }

    #[test]
    fn corrupt_file_is_backed_up_and_replaced_with_defaults() {
        let dir = tempdir();
        std::fs::write(settings_path(&dir), b"{ not json").unwrap();

        let (settings, outcome) = load(&dir).expect("load");
        assert_eq!(settings, GlobalSettings::default());
        match outcome {
            LoadOutcome::RecoveredFromCorruption { corrupt_backup } => {
                assert!(corrupt_backup.exists());
                let contents = std::fs::read_to_string(&corrupt_backup).unwrap();
                assert_eq!(contents, "{ not json");
            }
            other => panic!("expected RecoveredFromCorruption, got {other:?}"),
        }
        // And the file on disk is now valid defaults, not the corrupt content.
        let (reloaded, outcome2) = load(&dir).expect("reload");
        assert_eq!(outcome2, LoadOutcome::Loaded);
        assert_eq!(reloaded, GlobalSettings::default());
    }

    #[test]
    fn missing_sections_in_an_older_file_fall_back_to_defaults() {
        // Simulates a hand-rolled/older settings.json missing newer sections entirely.
        let dir = tempdir();
        std::fs::write(settings_path(&dir), br#"{"schemaVersion":1}"#).unwrap();

        let (settings, outcome) = load(&dir).expect("load");
        assert_eq!(outcome, LoadOutcome::Loaded);
        assert_eq!(settings.toolchain, ToolchainSettings::default());
        // `M10`: a file predating `updates`/`privacyNoticeAcknowledged` must not fail to
        // parse, and must not silently opt an existing install out of update checks or
        // skip a privacy disclosure it never actually showed.
        assert_eq!(settings.updates, UpdateSettings::default());
        assert!(settings.updates.check_for_updates);
        assert!(!settings.privacy_notice_acknowledged);
        assert_eq!(settings.claude, ClaudeSettings::default());
    }

    /// `M9`/`DATA-MODEL.md` §12: "`schemaVersion` lower than current: migrate in memory,
    /// re-write atomically, log the migration." `schemaVersion: 0` stands in for "a file
    /// from before this schema existed" — there's never actually been a shipped v0 (this
    /// app has only ever had schema v1), so this is the same kind of pre-v1 fixture
    /// `missing_sections_in_an_older_file_fall_back_to_defaults` already uses, just with an
    /// explicit lower version number to exercise the bump-and-rewrite path specifically.
    #[test]
    fn a_file_with_a_lower_schema_version_is_migrated_and_rewritten() {
        let dir = tempdir();
        std::fs::write(
            settings_path(&dir),
            br#"{"schemaVersion":0,"claude":{"permissionPolicy":"assisted","model":"opus","maxTurns":null,"showThinking":false,"showCostEstimate":true}}"#,
        )
        .unwrap();

        let (settings, outcome) = load(&dir).expect("load");
        assert_eq!(outcome, LoadOutcome::Migrated { from_version: 0 });
        assert_eq!(settings.schema_version, CURRENT_SCHEMA_VERSION);
        // Fields the v0 fixture actually set survive the migration...
        assert_eq!(settings.claude.model, "opus");
        // ...and sections it didn't mention at all still fall back to current defaults.
        assert_eq!(settings.toolchain, ToolchainSettings::default());

        // Re-reading confirms the migration was actually written back to disk, not just
        // returned in memory for this one call.
        let (reloaded, outcome2) = load(&dir).expect("reload");
        assert_eq!(outcome2, LoadOutcome::Loaded);
        assert_eq!(reloaded.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(reloaded.claude.model, "opus");
    }

    #[test]
    fn a_file_with_a_higher_schema_version_loads_but_is_left_untouched_on_disk() {
        let dir = tempdir();
        let raw = br#"{"schemaVersion":99,"claude":{"permissionPolicy":"guarded","model":"a-future-model","maxTurns":null,"showThinking":false,"showCostEstimate":true}}"#;
        std::fs::write(settings_path(&dir), raw).unwrap();

        let (settings, outcome) = load(&dir).expect("load");
        assert_eq!(outcome, LoadOutcome::NewerSchema { found_version: 99 });
        assert_eq!(settings.claude.model, "a-future-model");

        // The file on disk must be byte-identical to what was there before — a downgrade
        // must never silently discard fields a newer version wrote.
        let on_disk = std::fs::read(settings_path(&dir)).unwrap();
        assert_eq!(on_disk, raw);
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vibe-hw-settings-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
