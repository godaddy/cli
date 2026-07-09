use std::path::{Path, PathBuf};
use std::process::Command;

fn git_short_sha() -> String {
    Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        // `cli_engine::BuildInfo::version_string()` only omits the "(commit ...,
        // built ...)" suffix when commit AND date are both empty — since
        // GDDY_BUILD_DATE is always set, an empty commit here would still render
        // as the broken-looking "commit , built <date>".
        .unwrap_or_else(|| "unknown".to_owned())
}

/// Resolves `../.git` to the actual git admin directory to watch for HEAD
/// changes. In a plain checkout `../.git` is a directory; in a git worktree
/// it's a file containing `gitdir: <path>` pointing at the worktree's own
/// admin dir (which has its own per-worktree `HEAD`/`logs/HEAD`).
fn resolve_git_dir() -> Option<PathBuf> {
    let dot_git = Path::new("../.git");
    if dot_git.is_dir() {
        return Some(dot_git.to_path_buf());
    }
    let contents = std::fs::read_to_string(dot_git).ok()?;
    let gitdir = contents.strip_prefix("gitdir:")?.trim();
    let resolved = dot_git.parent()?.join(gitdir);
    resolved.is_dir().then_some(resolved)
}

fn main() {
    println!("cargo:rustc-env=GDDY_BUILD_COMMIT={}", git_short_sha());
    println!(
        "cargo:rustc-env=GDDY_BUILD_DATE={}",
        chrono::Utc::now().format("%Y-%m-%d")
    );
    // HEAD only changes on checkout/detach; logs/HEAD is appended to on every
    // commit/checkout/merge, so watch both to catch the branch tip moving.
    // Only emitted when the git dir resolves, so a missing/unreadable .git
    // (e.g. a tarball source export) just skips rebuild-on-commit rather than
    // failing the build.
    if let Some(git_dir) = resolve_git_dir() {
        println!("cargo:rerun-if-changed={}", git_dir.join("HEAD").display());
        println!(
            "cargo:rerun-if-changed={}",
            git_dir.join("logs/HEAD").display()
        );
    }
}
