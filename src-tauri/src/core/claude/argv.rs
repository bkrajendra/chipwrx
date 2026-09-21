//! Turn argv construction. Every flag here must match `CLI-CONTRACT.md` §1.4 exactly —
//! never invent one. `--bare` is never used (skips `CLAUDE.md` and OAuth credentials).

use crate::core::settings::PermissionPolicySetting;

/// First turn gets a fresh, app-generated `--session-id`; every later turn in the same
/// Claude session resumes it. `ARCHITECTURE.md` §4.2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionRef {
    New(String),
    Resume(String),
}

impl SessionRef {
    pub fn id(&self) -> &str {
        match self {
            SessionRef::New(id) | SessionRef::Resume(id) => id,
        }
    }
}

/// `CLI-CONTRACT.md` §1.4 policy mapping table (`FR-CHAT-4`). Unrestricted has no
/// `--permission-mode`/`--allowedTools` pair — it's `--dangerously-skip-permissions`
/// instead — so this returns `None` for that case rather than a meaningless pair.
fn mode_and_allowed_tools(policy: PermissionPolicySetting) -> Option<(&'static str, &'static str)> {
    match policy {
        PermissionPolicySetting::Guarded => Some(("acceptEdits", "Read,Edit,Write,Glob,Grep,Bash(pio *)")),
        PermissionPolicySetting::Assisted => Some(("auto", "Read,Edit,Write,Glob,Grep,Bash")),
        PermissionPolicySetting::Unrestricted => None,
    }
}

/// Builds the argv *following* the resolved program path and any `Resolution::extra_args`
/// (e.g. a `-3` Python selector — never applicable to `claude`, but the caller prepends
/// `extra_args` uniformly the same way `commands::project` does for `pio`).
///
/// `permission_prompts_none_supported` gates `--permission-prompts none`
/// (`ClaudeFeature::PermissionPromptsNone`, v2.1.259+) — omitted on older CLIs per
/// `CLI-CONTRACT.md` §1.4.
pub fn build_turn_args(
    prompt: &str,
    policy: PermissionPolicySetting,
    permission_prompts_none_supported: bool,
    model: &str,
    session: &SessionRef,
) -> Vec<String> {
    let mut args = vec![
        "-p".to_string(),
        prompt.to_string(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--include-partial-messages".into(),
    ];

    match mode_and_allowed_tools(policy) {
        Some((mode, tools)) => {
            args.push("--permission-mode".into());
            args.push(mode.into());
            args.push("--allowedTools".into());
            args.push(tools.into());
        }
        None => args.push("--dangerously-skip-permissions".into()),
    }

    if permission_prompts_none_supported {
        args.push("--permission-prompts".into());
        args.push("none".into());
    }

    args.push("--model".into());
    args.push(model.to_string());

    match session {
        SessionRef::New(id) => {
            args.push("--session-id".into());
            args.push(id.clone());
        }
        SessionRef::Resume(id) => {
            args.push("--resume".into());
            args.push(id.clone());
        }
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_maps_to_accept_edits_and_narrow_bash() {
        let args = build_turn_args(
            "add a blink sketch",
            PermissionPolicySetting::Guarded,
            true,
            "sonnet",
            &SessionRef::New("11111111-1111-1111-1111-111111111111".into()),
        );
        assert_eq!(
            args,
            vec![
                "-p",
                "add a blink sketch",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
                "--permission-mode",
                "acceptEdits",
                "--allowedTools",
                "Read,Edit,Write,Glob,Grep,Bash(pio *)",
                "--permission-prompts",
                "none",
                "--model",
                "sonnet",
                "--session-id",
                "11111111-1111-1111-1111-111111111111",
            ]
        );
    }

    #[test]
    fn assisted_maps_to_auto_and_general_bash() {
        let args = build_turn_args(
            "run the tests",
            PermissionPolicySetting::Assisted,
            true,
            "sonnet",
            &SessionRef::Resume("s1".into()),
        );
        assert!(args.windows(2).any(|w| w == ["--permission-mode", "auto"]));
        assert!(args.windows(2).any(|w| w == ["--allowedTools", "Read,Edit,Write,Glob,Grep,Bash"]));
        assert!(args.windows(2).any(|w| w == ["--resume", "s1"]));
        assert!(!args.iter().any(|a| a == "--session-id"));
    }

    #[test]
    fn unrestricted_uses_dangerously_skip_permissions_with_no_mode_or_allowed_tools() {
        let args = build_turn_args(
            "do anything",
            PermissionPolicySetting::Unrestricted,
            true,
            "sonnet",
            &SessionRef::Resume("s1".into()),
        );
        assert!(args.iter().any(|a| a == "--dangerously-skip-permissions"));
        assert!(!args.iter().any(|a| a == "--permission-mode"));
        assert!(!args.iter().any(|a| a == "--allowedTools"));
    }

    #[test]
    fn permission_prompts_none_omitted_on_older_cli() {
        let args = build_turn_args(
            "hello",
            PermissionPolicySetting::Guarded,
            false,
            "sonnet",
            &SessionRef::Resume("s1".into()),
        );
        assert!(!args.iter().any(|a| a == "--permission-prompts"));
    }

    #[test]
    fn never_passes_bare() {
        for policy in [
            PermissionPolicySetting::Guarded,
            PermissionPolicySetting::Assisted,
            PermissionPolicySetting::Unrestricted,
        ] {
            let args = build_turn_args("x", policy, true, "sonnet", &SessionRef::Resume("s1".into()));
            assert!(!args.iter().any(|a| a == "--bare"));
        }
    }

    #[test]
    fn always_passes_output_format_verbose_and_partial_messages_with_an_explicit_mode() {
        // The trap this whole app exists to avoid (SPEC.md G2 / CLAUDE.md landmine 1): a
        // `-p` turn with no permission mode writes nothing and returns prose silently.
        for policy in [PermissionPolicySetting::Guarded, PermissionPolicySetting::Assisted] {
            let args = build_turn_args("x", policy, true, "sonnet", &SessionRef::New("s1".into()));
            assert!(args.iter().any(|a| a == "--permission-mode"));
        }
        let unrestricted = build_turn_args(
            "x",
            PermissionPolicySetting::Unrestricted,
            true,
            "sonnet",
            &SessionRef::New("s1".into()),
        );
        assert!(unrestricted.iter().any(|a| a == "--dangerously-skip-permissions"));
    }
}
