//! `.vibe/project.json` — per-workspace `ProjectSettings` (`DATA-MODEL.md` §4), plus the
//! mandatory `.vibe/.gitignore` that keeps the app's own state out of the user's repo.

use super::types::{CreatedBy, DeviceSettings, ProjectClaudeSettings, ProjectPipelineSettings, ProjectSettings};
use std::io::Write;
use std::path::{Path, PathBuf};

const CURRENT_SCHEMA_VERSION: u32 = 1;

pub fn vibe_dir(workspace: &Path) -> PathBuf {
    workspace.join(".vibe")
}

/// `std::fs::canonicalize` on Windows always returns a `\\?\`-prefixed extended-length
/// path (e.g. `\\?\C:\Users\...`). That form is correct but ugly to display, and not every
/// external program's argv parsing handles it reliably (some editor CLI launchers don't
/// expect it and silently fail to open the right folder). Stored/shown workspace paths
/// should always be the ordinary form. A no-op on other platforms.
pub fn canonicalize_workspace(path: &Path) -> std::io::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    Ok(strip_windows_verbatim_prefix(canonical))
}

/// Same fix as `canonicalize_workspace`, applied to an already-stored path string rather
/// than a live filesystem lookup — for self-healing `projects.json` entries a pre-fix
/// version of the app wrote with the `\\?\` prefix still in them.
pub fn strip_windows_verbatim_prefix_str(path: &str) -> String {
    strip_windows_verbatim_prefix(PathBuf::from(path)).display().to_string()
}

fn strip_windows_verbatim_prefix(path: PathBuf) -> PathBuf {
    if !cfg!(windows) {
        return path;
    }
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path
    }
}

fn settings_path(workspace: &Path) -> PathBuf {
    vibe_dir(workspace).join("project.json")
}

pub fn new_settings(name: &str, created_by: CreatedBy) -> ProjectSettings {
    let trusted = created_by == CreatedBy::VibeHardware;
    ProjectSettings {
        schema_version: CURRENT_SCHEMA_VERSION,
        id: uuid::Uuid::new_v4().to_string(),
        name: name.to_string(),
        active_env: None,
        device: DeviceSettings::default(),
        claude: ProjectClaudeSettings::default(),
        pipeline: ProjectPipelineSettings::default(),
        trusted,
        trust_scan_at: None,
        created_by,
        created_at: chrono::Utc::now().to_rfc3339(),
    }
}

/// `None` when the workspace has no `.vibe/project.json` yet (a folder the app never
/// opened before). `Ok(Some(_))` is a successful read; corruption is recovered the same
/// way as every other store here — backed up, not silently discarded.
pub fn load(workspace: &Path) -> std::io::Result<Option<ProjectSettings>> {
    match std::fs::read(settings_path(workspace)) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(s) => Ok(Some(s)),
            Err(_) => {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let backup = vibe_dir(workspace).join(format!("project.json.corrupt-{ts}"));
                std::fs::rename(settings_path(workspace), &backup)?;
                Ok(None)
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn save(workspace: &Path, settings: &ProjectSettings) -> std::io::Result<()> {
    let dir = vibe_dir(workspace);
    std::fs::create_dir_all(&dir)?;
    ensure_vibe_gitignore(workspace)?;

    let path = settings_path(workspace);
    let tmp = dir.join(format!("project.json.tmp-{}", std::process::id()));
    let json = serde_json::to_vec_pretty(settings)?;
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&json)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// `DATA-MODEL.md` §2: "`.vibe/.gitignore` containing `*` is mandatory. Without it, the
/// first `git add -A` in the user's own repo commits their entire conversation history."
pub fn ensure_vibe_gitignore(workspace: &Path) -> std::io::Result<()> {
    let dir = vibe_dir(workspace);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(".gitignore");
    if !path.exists() {
        std::fs::write(&path, "*\n")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalize_workspace_strips_the_windows_verbatim_prefix() {
        let dir = tempdir("canon");
        let canonical = canonicalize_workspace(&dir).expect("canonicalize");
        assert!(
            !canonical.to_string_lossy().starts_with(r"\\?\"),
            "expected no \\\\?\\ prefix, got {}",
            canonical.display()
        );
        // Still resolves to the same real directory.
        assert!(canonical.is_dir());
    }

    #[test]
    fn strip_windows_verbatim_prefix_leaves_non_verbatim_paths_alone() {
        let p = PathBuf::from("/home/fay/greenhouse-sensor");
        assert_eq!(strip_windows_verbatim_prefix(p.clone()), p);
    }

    #[cfg(windows)]
    #[test]
    fn strip_windows_verbatim_prefix_strips_drive_and_unc_forms() {
        assert_eq!(
            strip_windows_verbatim_prefix(PathBuf::from(r"\\?\C:\Users\fay\proj")),
            PathBuf::from(r"C:\Users\fay\proj")
        );
        assert_eq!(
            strip_windows_verbatim_prefix(PathBuf::from(r"\\?\UNC\server\share\proj")),
            PathBuf::from(r"\\server\share\proj")
        );
    }

    fn tempdir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vibe-hw-workspace-test-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn no_project_json_yet_loads_as_none() {
        let dir = tempdir("none");
        assert_eq!(load(&dir).unwrap(), None);
    }

    #[test]
    fn round_trips_and_writes_the_gitignore() {
        let dir = tempdir("roundtrip");
        let settings = new_settings("greenhouse-sensor", CreatedBy::VibeHardware);
        save(&dir, &settings).expect("save");

        let loaded = load(&dir).expect("load").expect("some");
        assert_eq!(loaded, settings);
        assert!(loaded.trusted, "app-created workspaces are trusted by default");

        let gitignore = std::fs::read_to_string(dir.join(".vibe").join(".gitignore")).unwrap();
        assert_eq!(gitignore, "*\n");
    }

    #[test]
    fn opened_workspaces_default_to_untrusted() {
        let settings = new_settings("someone-elses-project", CreatedBy::Opened);
        assert!(!settings.trusted);
    }

    #[test]
    fn each_new_settings_call_gets_a_fresh_id() {
        let a = new_settings("x", CreatedBy::VibeHardware);
        let b = new_settings("x", CreatedBy::VibeHardware);
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn gitignore_is_not_overwritten_if_user_edited_it() {
        let dir = tempdir("gitignore-preserved");
        std::fs::create_dir_all(dir.join(".vibe")).unwrap();
        std::fs::write(dir.join(".vibe").join(".gitignore"), "*\n# custom note\n").unwrap();

        ensure_vibe_gitignore(&dir).unwrap();
        let content = std::fs::read_to_string(dir.join(".vibe").join(".gitignore")).unwrap();
        assert_eq!(content, "*\n# custom note\n");
    }

    #[test]
    fn corrupt_project_json_is_backed_up() {
        let dir = tempdir("corrupt");
        std::fs::create_dir_all(dir.join(".vibe")).unwrap();
        std::fs::write(settings_path(&dir), b"{ not json").unwrap();

        let loaded = load(&dir).expect("load");
        assert_eq!(loaded, None);
        assert!(!settings_path(&dir).exists());
    }
}
