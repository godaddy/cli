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

fn main() {
    println!("cargo:rustc-env=GDDY_BUILD_COMMIT={}", git_short_sha());
    println!(
        "cargo:rustc-env=GDDY_BUILD_DATE={}",
        chrono::Utc::now().format("%Y-%m-%d")
    );
    // .git/HEAD only changes on checkout/detach; .git/logs/HEAD is appended to
    // on every commit/checkout/merge, so watch both to catch the branch tip moving.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/logs/HEAD");
}
