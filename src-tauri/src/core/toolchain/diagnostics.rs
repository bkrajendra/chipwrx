//! Diagnostics bundle export (`TOOLCHAIN-SETUP.md` §10, `NFR-R1`). One action, one zip.
//!
//! Deliberately takes every text member as an already-fetched `Option<&str>` rather than
//! spawning anything itself — the commands layer gathers `pio system info`, `pio settings
//! get`, `claude doctor`, and the on-disk log, so this stays a pure, unit-testable
//! "assemble and redact a zip" function. Members that aren't available yet at this
//! milestone (no workspace concept until M2, no build history until M5) are simply `None`
//! and the bundle notes them as unavailable rather than failing.

use super::types::DoctorReport;
use crate::core::redact::redact;
use crate::core::settings::GlobalSettings;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct DiagnosticsInputs<'a> {
    pub doctor_report: &'a DoctorReport,
    pub global_settings: &'a GlobalSettings,
    pub pio_system_info_json: Option<&'a str>,
    pub pio_settings_text: Option<&'a str>,
    pub claude_doctor_text: Option<&'a str>,
    pub app_log: Option<&'a str>,
    pub active_platformio_ini: Option<&'a str>,
    pub last_build_log: Option<&'a str>,
    pub app_version: &'a str,
    pub git_sha: Option<&'a str>,
    /// Whether to also redact `SER=...` device serial numbers (`core::redact`).
    pub redact_serials: bool,
}

fn redact_home(text: &str, home: &Path) -> String {
    let home_str = home.to_string_lossy();
    if home_str.is_empty() {
        text.to_string()
    } else {
        text.replace(home_str.as_ref(), "~")
    }
}

fn env_txt() -> String {
    let path = std::env::var("PATH").unwrap_or_default();
    let http_proxy = std::env::var("HTTP_PROXY").or_else(|_| std::env::var("http_proxy")).unwrap_or_default();
    let https_proxy = std::env::var("HTTPS_PROXY").or_else(|_| std::env::var("https_proxy")).unwrap_or_default();
    format!(
        "OS={}\nARCH={}\nHTTP_PROXY={http_proxy}\nHTTPS_PROXY={https_proxy}\nPATH={path}\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

/// Builds the zip in memory and writes it to `<dest_dir>/vibe-hardware-diagnostics-<unix
/// ts>.zip`, returning the written path. `home` is used only to redact absolute paths in
/// `app-settings.json` down to `~` — it never needs to be a real, existing directory.
pub fn build_bundle(
    inputs: &DiagnosticsInputs,
    home: &Path,
    dest_dir: &Path,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dest_dir)?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dest = dest_dir.join(format!("vibe-hardware-diagnostics-{ts}.zip"));

    let file = std::fs::File::create(&dest)?;
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut write_member = |name: &str, content: &str| -> std::io::Result<()> {
        zip.start_file(name, options)?;
        zip.write_all(redact(content, inputs.redact_serials).as_bytes())?;
        Ok(())
    };

    let doctor_json = serde_json::to_string_pretty(inputs.doctor_report).unwrap_or_default();
    write_member("doctor.json", &doctor_json)?;

    let settings_json = serde_json::to_string_pretty(inputs.global_settings).unwrap_or_default();
    write_member("app-settings.json", &redact_home(&settings_json, home))?;

    write_member(
        "pio-system-info.json",
        inputs.pio_system_info_json.unwrap_or("(unavailable — PlatformIO not resolved)"),
    )?;
    write_member(
        "pio-settings.txt",
        inputs.pio_settings_text.unwrap_or("(unavailable — PlatformIO not resolved)"),
    )?;
    write_member(
        "claude-doctor.txt",
        inputs.claude_doctor_text.unwrap_or("(unavailable — Claude Code not resolved)"),
    )?;
    write_member("app.log", inputs.app_log.unwrap_or("(unavailable)"))?;
    write_member(
        "platformio.ini",
        inputs
            .active_platformio_ini
            .unwrap_or("(unavailable — no active project)"),
    )?;
    write_member(
        "last-build.log",
        inputs.last_build_log.unwrap_or("(unavailable — no build has run yet)"),
    )?;

    let mut env = env_txt();
    env.push_str(&format!("APP_VERSION={}\n", inputs.app_version));
    env.push_str(&format!("GIT_SHA={}\n", inputs.git_sha.unwrap_or("unknown")));
    write_member("env.txt", &env)?;

    zip.finish()?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::toolchain::types::ProbeResult;

    fn sample_report() -> DoctorReport {
        DoctorReport {
            claude_binary: ProbeResult::Missing { install_available: true },
            claude_auth: ProbeResult::Probing,
            claude_capabilities: vec![],
            pio_binary: ProbeResult::Missing { install_available: true },
            pio_core_dir: ProbeResult::Probing,
            python: ProbeResult::Probing,
            network_registry: ProbeResult::Probing,
            serial_permissions: ProbeResult::Ok { version: String::new(), path: None, detail: None },
            git: ProbeResult::Probing,
            probed_at: "2026-09-21T00:00:00Z".into(),
        }
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vibe-hw-diag-test-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn builds_a_zip_with_every_member_and_redacts_secrets() {
        let dest_dir = scratch_dir("bundle");
        let settings = GlobalSettings::default();
        let report = sample_report();
        let claude_doctor = "ANTHROPIC_API_KEY=sk-ant-verysecretvalue1234\nsome other line\n";

        let inputs = DiagnosticsInputs {
            doctor_report: &report,
            global_settings: &settings,
            pio_system_info_json: Some(r#"{"core_version":{"value":"6.2.0"}}"#),
            pio_settings_text: Some("enable_telemetry = No\n"),
            claude_doctor_text: Some(claude_doctor),
            app_log: Some("[info] app started\n"),
            active_platformio_ini: None,
            last_build_log: None,
            app_version: "0.1.0",
            git_sha: Some("deadbeef"),
            redact_serials: false,
        };

        let path = build_bundle(&inputs, Path::new("/Users/raj"), &dest_dir).expect("build bundle");
        assert!(path.exists());
        assert!(path.file_name().unwrap().to_string_lossy().starts_with("vibe-hardware-diagnostics-"));

        let file = std::fs::File::open(&path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();

        let mut names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "app-settings.json",
                "app.log",
                "claude-doctor.txt",
                "doctor.json",
                "env.txt",
                "last-build.log",
                "pio-settings.txt",
                "pio-system-info.json",
                "platformio.ini",
            ]
        );

        let mut claude_doctor_out = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("claude-doctor.txt").unwrap(),
            &mut claude_doctor_out,
        )
        .unwrap();
        assert!(!claude_doctor_out.contains("verysecretvalue1234"));
        assert!(claude_doctor_out.contains("some other line"));

        let mut env_out = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("env.txt").unwrap(), &mut env_out).unwrap();
        assert!(env_out.contains("APP_VERSION=0.1.0"));
        assert!(env_out.contains("GIT_SHA=deadbeef"));

        let _ = std::fs::remove_dir_all(&dest_dir);
    }

    #[test]
    fn redacts_home_directory_in_settings_json() {
        let dest_dir = scratch_dir("home-redact");
        let mut settings = GlobalSettings::default();
        settings.toolchain.claude_path = Some("/Users/raj/.local/bin/claude".into());
        let report = sample_report();

        let inputs = DiagnosticsInputs {
            doctor_report: &report,
            global_settings: &settings,
            pio_system_info_json: None,
            pio_settings_text: None,
            claude_doctor_text: None,
            app_log: None,
            active_platformio_ini: None,
            last_build_log: None,
            app_version: "0.1.0",
            git_sha: None,
            redact_serials: false,
        };

        let path = build_bundle(&inputs, Path::new("/Users/raj"), &dest_dir).expect("build bundle");
        let file = std::fs::File::open(&path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut settings_out = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("app-settings.json").unwrap(),
            &mut settings_out,
        )
        .unwrap();
        assert!(!settings_out.contains("/Users/raj"));
        assert!(settings_out.contains("~/.local/bin/claude"));

        let _ = std::fs::remove_dir_all(&dest_dir);
    }
}
