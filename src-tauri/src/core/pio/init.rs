//! `pio project init` (`CLI-CONTRACT.md` §4.1, `FR-PROJ-1/2`).

use crate::error::AppError;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use ts_rs::TS;

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectRequest {
    pub parent_dir: String,
    pub name: String,
    pub board_id: String,
    pub framework: String,
    pub sample_code: bool,
    pub init_git: bool,
    pub generate_claude_md: bool,
    /// Repeatable `-O name=value` project options.
    pub extra_options: Vec<(String, String)>,
}

pub fn project_dir(req: &CreateProjectRequest) -> PathBuf {
    Path::new(&req.parent_dir).join(&req.name)
}

/// `NFR-S4`: validated against an allowlist before it ever reaches a path join or a
/// spawned command — a project name is user-typed text, not a trusted identifier.
pub fn validate_name(name: &str) -> Result<(), AppError> {
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\'])
        || name.contains(':')
        || name.chars().any(|c| c.is_control());
    if bad {
        Err(AppError::Io {
            message: format!("'{name}' is not a valid project name"),
        })
    } else {
        Ok(())
    }
}

/// Builds `pio project init`'s args (not including the resolved `pio` program itself, or
/// any `Resolution::extra_args` — the caller prepends those, same as every other `pio`
/// invocation).
pub fn build_init_args(req: &CreateProjectRequest, dir: &Path) -> Vec<String> {
    let mut args = vec![
        "project".to_string(),
        "init".to_string(),
        "-d".to_string(),
        dir.display().to_string(),
        "-b".to_string(),
        req.board_id.clone(),
    ];
    if req.sample_code {
        args.push("--sample-code".to_string());
    }
    args.push("-O".to_string());
    args.push(format!("framework={}", req.framework));
    for (k, v) in &req.extra_options {
        args.push("-O".to_string());
        args.push(format!("{k}={v}"));
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request() -> CreateProjectRequest {
        CreateProjectRequest {
            parent_dir: "/home/fay/projects".into(),
            name: "greenhouse-sensor".into(),
            board_id: "esp32dev".into(),
            framework: "arduino".into(),
            sample_code: true,
            init_git: true,
            generate_claude_md: true,
            extra_options: vec![("monitor_speed".into(), "115200".into())],
        }
    }

    #[test]
    fn project_dir_joins_parent_and_name() {
        let dir = project_dir(&sample_request());
        assert_eq!(dir, Path::new("/home/fay/projects/greenhouse-sensor"));
    }

    #[test]
    fn build_init_args_matches_cli_contract_shape() {
        let req = sample_request();
        let dir = project_dir(&req);
        let args = build_init_args(&req, &dir);
        assert_eq!(
            args,
            vec![
                "project".to_string(),
                "init".to_string(),
                "-d".to_string(),
                dir.display().to_string(),
                "-b".to_string(),
                "esp32dev".to_string(),
                "--sample-code".to_string(),
                "-O".to_string(),
                "framework=arduino".to_string(),
                "-O".to_string(),
                "monitor_speed=115200".to_string(),
            ]
        );
    }

    #[test]
    fn build_init_args_omits_sample_code_flag_when_false() {
        let mut req = sample_request();
        req.sample_code = false;
        let dir = project_dir(&req);
        let args = build_init_args(&req, &dir);
        assert!(!args.contains(&"--sample-code".to_string()));
    }

    #[test]
    fn rejects_empty_and_traversal_names() {
        assert!(validate_name("").is_err());
        assert!(validate_name(".").is_err());
        assert!(validate_name("..").is_err());
        assert!(validate_name("../escape").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("a\\b").is_err());
        assert!(validate_name("C:foo").is_err());
    }

    #[test]
    fn accepts_ordinary_names() {
        assert!(validate_name("greenhouse-sensor").is_ok());
        assert!(validate_name("my_project_2").is_ok());
    }
}
