//! `projects.json` — the app-wide project registry (`DATA-MODEL.md` §5, `FR-PROJ-4`).
//! Reconciled by `id` first, never silently drops an entry whose path went missing.

use super::types::{ProjectEntry, ProjectRegistry, ProjectRegistryEntry};
use std::io::Write;
use std::path::{Path, PathBuf};

const CURRENT_SCHEMA_VERSION: u32 = 1;
const FILE_NAME: &str = "projects.json";

fn registry_path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE_NAME)
}

pub fn load(config_dir: &Path) -> std::io::Result<ProjectRegistry> {
    match std::fs::read(registry_path(config_dir)) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(r) => Ok(r),
            Err(_) => {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let backup = config_dir.join(format!("{FILE_NAME}.corrupt-{ts}"));
                let _ = std::fs::rename(registry_path(config_dir), &backup);
                Ok(ProjectRegistry {
                    schema_version: CURRENT_SCHEMA_VERSION,
                    projects: vec![],
                })
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ProjectRegistry {
            schema_version: CURRENT_SCHEMA_VERSION,
            projects: vec![],
        }),
        Err(e) => Err(e),
    }
}

pub fn save(config_dir: &Path, registry: &ProjectRegistry) -> std::io::Result<()> {
    std::fs::create_dir_all(config_dir)?;
    let path = registry_path(config_dir);
    let tmp = config_dir.join(format!("{FILE_NAME}.tmp-{}", std::process::id()));
    let json = serde_json::to_vec_pretty(registry)?;
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&json)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Inserts `entry`, or updates the existing entry with the same `id` in place (reconciled
/// by `id`, not `path` — a moved folder updates its existing row rather than duplicating).
pub fn upsert(registry: &mut ProjectRegistry, entry: ProjectRegistryEntry) {
    if let Some(existing) = registry.projects.iter_mut().find(|p| p.id == entry.id) {
        *existing = entry;
    } else {
        registry.projects.push(entry);
    }
}

pub fn forget(registry: &mut ProjectRegistry, id: &str) {
    registry.projects.retain(|p| p.id != id);
}

pub fn find<'a>(registry: &'a ProjectRegistry, id: &str) -> Option<&'a ProjectRegistryEntry> {
    registry.projects.iter().find(|p| p.id == id)
}

/// Computes `exists` for every entry — `FR-PROJ-4`: a missing path is flagged, never
/// dropped from the list.
pub fn to_ipc_entries(registry: &ProjectRegistry) -> Vec<ProjectEntry> {
    registry
        .projects
        .iter()
        .map(|p| ProjectEntry {
            id: p.id.clone(),
            name: p.name.clone(),
            path: p.path.clone(),
            board_id: p.board_id.clone(),
            active_env: p.active_env.clone(),
            last_opened: p.last_opened.clone(),
            last_build_ok: p.last_build_ok,
            exists: Path::new(&p.path).exists(),
            trusted: p.trusted,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vibe-hw-registry-test-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_entry(id: &str, path: &str) -> ProjectRegistryEntry {
        ProjectRegistryEntry {
            id: id.into(),
            name: "greenhouse-sensor".into(),
            path: path.into(),
            board_id: Some("esp32dev".into()),
            active_env: Some("esp32dev".into()),
            last_opened: Some("2026-09-20T09:12:44Z".into()),
            last_build_ok: Some(true),
            trusted: true,
        }
    }

    #[test]
    fn missing_file_loads_as_empty_registry() {
        let dir = tempdir("missing");
        let r = load(&dir).expect("load");
        assert!(r.projects.is_empty());
        assert_eq!(r.schema_version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn round_trips_through_save_and_load() {
        let dir = tempdir("roundtrip");
        let mut r = ProjectRegistry {
            schema_version: CURRENT_SCHEMA_VERSION,
            projects: vec![],
        };
        upsert(&mut r, sample_entry("id-1", "/home/fay/greenhouse-sensor"));
        save(&dir, &r).expect("save");

        let loaded = load(&dir).expect("load");
        assert_eq!(loaded, r);
    }

    #[test]
    fn upsert_reconciles_by_id_not_path() {
        let mut r = ProjectRegistry {
            schema_version: CURRENT_SCHEMA_VERSION,
            projects: vec![],
        };
        upsert(&mut r, sample_entry("id-1", "/old/path"));
        // Same id, different path (folder moved) — must update in place, not duplicate.
        upsert(&mut r, sample_entry("id-1", "/new/path"));

        assert_eq!(r.projects.len(), 1);
        assert_eq!(r.projects[0].path, "/new/path");
    }

    #[test]
    fn upsert_adds_a_new_row_for_a_different_id() {
        let mut r = ProjectRegistry {
            schema_version: CURRENT_SCHEMA_VERSION,
            projects: vec![],
        };
        upsert(&mut r, sample_entry("id-1", "/a"));
        upsert(&mut r, sample_entry("id-2", "/b"));
        assert_eq!(r.projects.len(), 2);
    }

    #[test]
    fn forget_removes_only_the_matching_entry() {
        let mut r = ProjectRegistry {
            schema_version: CURRENT_SCHEMA_VERSION,
            projects: vec![],
        };
        upsert(&mut r, sample_entry("id-1", "/a"));
        upsert(&mut r, sample_entry("id-2", "/b"));
        forget(&mut r, "id-1");
        assert_eq!(r.projects.len(), 1);
        assert_eq!(r.projects[0].id, "id-2");
    }

    #[test]
    fn missing_paths_are_flagged_not_dropped() {
        let mut r = ProjectRegistry {
            schema_version: CURRENT_SCHEMA_VERSION,
            projects: vec![],
        };
        let real_dir = tempdir("exists-check");
        upsert(&mut r, sample_entry("id-1", &real_dir.display().to_string()));
        upsert(&mut r, sample_entry("id-2", "/definitely/does/not/exist/anywhere"));

        let entries = to_ipc_entries(&r);
        let real = entries.iter().find(|e| e.id == "id-1").unwrap();
        let gone = entries.iter().find(|e| e.id == "id-2").unwrap();
        assert!(real.exists);
        assert!(!gone.exists);
        // Still present in the list even though its path is gone.
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn corrupt_file_is_backed_up_and_registry_starts_empty() {
        let dir = tempdir("corrupt");
        std::fs::write(registry_path(&dir), b"{ not json").unwrap();

        let r = load(&dir).expect("load");
        assert!(r.projects.is_empty());

        // The corrupt content was moved aside, not left in place or deleted outright.
        assert!(!registry_path(&dir).exists());
        let backups: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(&format!("{FILE_NAME}.corrupt-")))
            .collect();
        assert_eq!(backups.len(), 1);
    }
}
