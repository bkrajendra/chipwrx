//! `pio run` argv construction (`CLI-CONTRACT.md` §5.1, `FR-BUILD-1/2/7`). Every action's
//! argv matches the "App invocations" table there exactly — never an invented flag.

use std::path::Path;

pub fn build_args(dir: &Path, env: &str) -> Vec<String> {
    vec!["run".into(), "-d".into(), dir.display().to_string(), "-e".into(), env.into()]
}

/// `port` is `None` when the app has no `PortBroker` yet to source one from
/// (`SPEC.md` §8 open question 17) — `--upload-port` is simply omitted, and PlatformIO
/// auto-detects.
pub fn upload_args(dir: &Path, env: &str, port: Option<&str>) -> Vec<String> {
    let mut args = build_args(dir, env);
    args.push("-t".into());
    args.push("upload".into());
    if let Some(p) = port {
        args.push("--upload-port".into());
        args.push(p.into());
    }
    args
}

pub fn target_args(dir: &Path, env: &str, target: &str) -> Vec<String> {
    let mut args = build_args(dir, env);
    args.push("-t".into());
    args.push(target.into());
    args
}

pub fn list_targets_args(dir: &Path, env: &str) -> Vec<String> {
    let mut args = build_args(dir, env);
    args.push("--list-targets".into());
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dir() -> PathBuf {
        PathBuf::from("/home/fay/greenhouse-sensor")
    }

    #[test]
    fn build_matches_the_cli_contract_shape() {
        assert_eq!(build_args(&dir(), "esp32dev"), vec!["run", "-d", "/home/fay/greenhouse-sensor", "-e", "esp32dev"]);
    }

    #[test]
    fn upload_with_a_port_matches_the_cli_contract_shape() {
        assert_eq!(
            upload_args(&dir(), "esp32dev", Some("/dev/ttyUSB0")),
            vec!["run", "-d", "/home/fay/greenhouse-sensor", "-e", "esp32dev", "-t", "upload", "--upload-port", "/dev/ttyUSB0"]
        );
    }

    #[test]
    fn upload_without_a_port_omits_the_flag_entirely() {
        let args = upload_args(&dir(), "esp32dev", None);
        assert!(!args.iter().any(|a| a == "--upload-port"));
        assert_eq!(args, vec!["run", "-d", "/home/fay/greenhouse-sensor", "-e", "esp32dev", "-t", "upload"]);
    }

    #[test]
    fn target_args_matches_the_cli_contract_shape() {
        assert_eq!(
            target_args(&dir(), "esp32dev", "clean"),
            vec!["run", "-d", "/home/fay/greenhouse-sensor", "-e", "esp32dev", "-t", "clean"]
        );
    }

    #[test]
    fn list_targets_args_matches_the_cli_contract_shape() {
        assert_eq!(
            list_targets_args(&dir(), "esp32dev"),
            vec!["run", "-d", "/home/fay/greenhouse-sensor", "-e", "esp32dev", "--list-targets"]
        );
    }
}
