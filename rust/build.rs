use std::process::Command;

fn git_short_sha() -> String {
    Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}

fn main() {
    println!("cargo:rustc-env=GDDY_BUILD_COMMIT={}", git_short_sha());
    println!(
        "cargo:rustc-env=GDDY_BUILD_DATE={}",
        chrono::Utc::now().format("%Y-%m-%d")
    );
    println!("cargo:rerun-if-changed=../.git/HEAD");
}
