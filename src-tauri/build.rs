use std::process::Command;

/// `NFR-D3`: "version and git SHA surfaced in About and in the diagnostics bundle." Read at
/// build time rather than runtime so a release build carries the SHA it was actually built
/// from even when the shipped binary runs on a machine with no `.git` at all (an installed
/// app, or a source tarball). CI checkouts (`actions/checkout`) always have `.git`, so this
/// only falls back to `"unknown"` for a from-scratch source tree with no git history.
fn git_sha() -> String {
    Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn main() {
    println!("cargo:rustc-env=VIBE_GIT_SHA={}", git_sha());
    // Best-effort: re-run when HEAD moves to a different commit or branch. Missing in a
    // source tarball with no `.git`, which is fine — the SHA was already baked in above.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/index");

    tauri_build::build()
}
