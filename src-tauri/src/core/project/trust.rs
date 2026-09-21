//! Workspace trust scan (`NFR-S5`, `ARCHITECTURE.md` §9 rule 4). `claude -p` runs a
//! project's hooks and connects its MCP servers with no prompt of its own — this scans an
//! adopted folder for exactly those before the first turn, so the app can show what it
//! found rather than silently letting arbitrary code run.

use super::types::TrustScan;
use std::path::Path;

/// Scans `workspace` for `.claude/settings.json` hooks, `.mcp.json` MCP servers, and
/// `.claude/agents/*`. Never fails — an unreadable or malformed file just means nothing
/// was found there, which errs toward showing the trust prompt rather than skipping it.
pub fn scan(workspace: &Path) -> TrustScan {
    TrustScan {
        hooks: scan_hooks(workspace),
        mcp_servers: scan_mcp_servers(workspace),
        agents: scan_agents(workspace),
    }
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn scan_hooks(workspace: &Path) -> Vec<String> {
    let Some(json) = read_json(&workspace.join(".claude").join("settings.json")) else {
        return Vec::new();
    };
    let Some(hooks) = json.get("hooks").and_then(|h| h.as_object()) else {
        return Vec::new();
    };
    let mut names: Vec<String> = hooks.keys().cloned().collect();
    names.sort();
    names
}

fn scan_mcp_servers(workspace: &Path) -> Vec<String> {
    let Some(json) = read_json(&workspace.join(".mcp.json")) else {
        return Vec::new();
    };
    let Some(servers) = json.get("mcpServers").and_then(|s| s.as_object()) else {
        return Vec::new();
    };
    let mut names: Vec<String> = servers.keys().cloned().collect();
    names.sort();
    names
}

fn scan_agents(workspace: &Path) -> Vec<String> {
    let dir = workspace.join(".claude").join("agents");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("vibe-hw-trust-test-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn empty_workspace_scans_clean() {
        let dir = tempdir("empty");
        let scan = scan(&dir);
        assert!(scan.is_empty());
    }

    #[test]
    fn finds_hooks_mcp_servers_and_agents() {
        let dir = tempdir("full");
        std::fs::create_dir_all(dir.join(".claude").join("agents")).unwrap();
        std::fs::write(
            dir.join(".claude").join("settings.json"),
            r#"{"hooks":{"SessionStart":[{}],"PreToolUse":[{}]}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join(".mcp.json"),
            r#"{"mcpServers":{"filesystem":{"command":"npx"},"weather":{"command":"npx"}}}"#,
        )
        .unwrap();
        std::fs::write(dir.join(".claude").join("agents").join("reviewer.md"), "# Reviewer").unwrap();
        std::fs::write(dir.join(".claude").join("agents").join("planner.md"), "# Planner").unwrap();

        let scan = scan(&dir);
        assert!(!scan.is_empty());
        assert_eq!(scan.hooks, vec!["PreToolUse", "SessionStart"]);
        assert_eq!(scan.mcp_servers, vec!["filesystem", "weather"]);
        assert_eq!(scan.agents, vec!["planner", "reviewer"]);
    }

    #[test]
    fn malformed_settings_json_is_treated_as_nothing_found_not_an_error() {
        let dir = tempdir("malformed");
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join(".claude").join("settings.json"), "{ not json").unwrap();

        let scan = scan(&dir);
        assert!(scan.hooks.is_empty());
    }

    #[test]
    fn settings_json_without_hooks_key_is_clean() {
        let dir = tempdir("no-hooks-key");
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join(".claude").join("settings.json"), r#"{"model":"sonnet"}"#).unwrap();

        let scan = scan(&dir);
        assert!(scan.hooks.is_empty());
    }
}
