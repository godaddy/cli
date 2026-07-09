//! `gddy update` — check for and install newer `gddy` releases.
//!
//! `gddy` never updates itself silently. `gddy update check` reports whether a
//! newer release exists; `gddy update apply` downloads it, verifies its
//! SHA-256 checksum against the release's published checksums file, and swaps
//! the running binary in place via [`self_replace`].
//!
//! A lightweight passive notice (see [`maybe_spawn_background_refresh`] and
//! [`maybe_print_update_notice`], wired into `main.rs`'s `pre_run`/
//! `on_shutdown` hooks) checks GitHub in the background at most once every
//! [`CACHE_TTL`] and prints a one-line heads-up on stderr when a newer
//! release is cached — never blocking, never auto-installing.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, GroupSpec, Module, RuntimeCommandSpec,
    RuntimeGroupSpec, Tier,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::output_schema::output_schema;

output_schema!(UpdateCheckResult {
    "currentVersion": "string";
    "latestVersion": "string";
    "updateAvailable": "bool";
});

output_schema!(UpdateApplyResult {
    "previousVersion": "string";
    "newVersion": "string";
    "status": "string";
});

const REPO: &str = "godaddy/cli";
const CACHE_FILE_NAME: &str = "update-check.json";
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const BACKGROUND_TIMEOUT: Duration = Duration::from_secs(2);
const FOREGROUND_TIMEOUT: Duration = Duration::from_secs(15);
const CHECKSUMS_FILE_NAME: &str = "gddy-checksums-sha256.txt";

/// Set once `update apply` has replaced the running binary, so the
/// `on_shutdown` notice doesn't tell the user to update again this same
/// invocation — the freshly-refreshed cache would otherwise make it look
/// like an update is still available (the *running* process is still the
/// old version even though the binary on disk was just swapped).
static UPDATE_APPLIED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpdateCache {
    checked_at: String,
    latest_version: String,
    /// The exact release tag (e.g. `v1.2.3`), used to build download URLs —
    /// `latest_version` is normalized for comparison/display and may not
    /// round-trip back to the tag's exact spelling.
    tag_name: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
}

pub fn module() -> Module {
    Module::new("Admin", |_ctx| {
        RuntimeGroupSpec::new(
            GroupSpec::new("update", "Check for and install gddy updates").with_long(
                "Checks GitHub Releases for a newer gddy build and can install it in \
                 place.\n\
                 \n\
                 • check — report whether a newer version is available (no changes made)\n\
                 • apply — download, verify, and install the latest release\n\
                 \n\
                 gddy never updates itself silently — run `gddy update apply` when \
                 you're ready.",
            ),
        )
        .with_command(check_command())
        .with_command(apply_command())
    })
}

fn check_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new(
        CommandSpec::new("check", "Check whether a newer gddy version is available")
            .with_long(
                "Queries GitHub Releases for the latest gddy release and compares it \
                 against the running version. Downloads and installs nothing — use \
                 `gddy update apply` to install an available update.",
            )
            .with_system("update")
            .with_tier(Tier::Read)
            .no_auth(true)
            .with_output_schema::<UpdateCheckResult>()
            .with_default_fields("currentVersion,latestVersion,updateAvailable"),
        |_cred, _args| async move {
            let client = http_client()?;
            let cache = refresh_cache(&client, FOREGROUND_TIMEOUT).await?;
            let current = current_version();
            let latest = parse_version(&cache.latest_version).ok_or_else(|| {
                CliCoreError::message(format!(
                    "unexpected version format from GitHub: {}",
                    cache.latest_version
                ))
            })?;
            Ok(CommandResult::new(json!({
                "currentVersion": current.to_string(),
                "latestVersion": latest.to_string(),
                "updateAvailable": latest > current,
            })))
        },
    )
}

fn apply_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new(
        CommandSpec::new("apply", "Download and install the latest gddy release")
            .with_long(
                "Downloads the latest gddy release for this platform, verifies its \
                 SHA-256 checksum against the release's published checksums file, and \
                 replaces the currently running binary in place.\n\
                 \n\
                 Run `gddy update check` first to preview whether an update is \
                 available without installing anything.",
            )
            .with_system("update")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .no_auth(true)
            .with_output_schema::<UpdateApplyResult>()
            .with_default_fields("previousVersion,newVersion,status"),
        |_cred, _args| async move { Ok(CommandResult::new(run_apply().await?)) },
    )
}

async fn run_apply() -> Result<serde_json::Value, CliCoreError> {
    let current = current_version();
    let client = http_client()?;
    let cache = refresh_cache(&client, FOREGROUND_TIMEOUT).await?;
    let latest = parse_version(&cache.latest_version).ok_or_else(|| {
        CliCoreError::message(format!(
            "unexpected version format from GitHub: {}",
            cache.latest_version
        ))
    })?;
    if latest <= current {
        return Ok(json!({
            "previousVersion": current.to_string(),
            "newVersion": current.to_string(),
            "status": "already up to date",
        }));
    }

    let target = target_triple()?;
    let asset = asset_name(target);
    let base_url = format!(
        "https://github.com/{REPO}/releases/download/{}",
        cache.tag_name
    );

    let archive_bytes = download(&client, &format!("{base_url}/{asset}")).await?;
    let checksums_text =
        download_text(&client, &format!("{base_url}/{CHECKSUMS_FILE_NAME}")).await?;
    verify_checksum(&checksums_text, &asset, &archive_bytes)?;

    let bin = bin_name();
    let extracted = if target.contains("windows") {
        extract_zip(&archive_bytes, bin)?
    } else {
        extract_tar_gz(&archive_bytes, bin)?
    };

    let tmp_path = write_temp_binary(&extracted)?;
    let replaced = self_replace::self_replace(&tmp_path)
        .map_err(|e| CliCoreError::message(format!("failed to replace running binary: {e}")));
    let _ = std::fs::remove_file(&tmp_path);
    replaced?;
    UPDATE_APPLIED.store(true, Ordering::Relaxed);

    Ok(json!({
        "previousVersion": current.to_string(),
        "newVersion": latest.to_string(),
        "status": "updated",
    }))
}

/// Spawns a best-effort background refresh of the update cache if it is
/// missing or stale. Never blocks the calling command and swallows all
/// errors — this is purely advisory, feeding the next invocation's passive
/// notice, never the current one.
pub fn maybe_spawn_background_refresh() {
    if !should_show_notice() {
        return;
    }
    let Some(path) = cache_path() else { return };
    let cache = load_cache(&path);
    if !is_stale(&cache) {
        return;
    }
    tokio::spawn(async move {
        if let Ok(client) = http_client() {
            let _ = refresh_cache(&client, BACKGROUND_TIMEOUT).await;
        }
    });
}

/// Prints a one-line "update available" notice to stderr based on the
/// previously cached check, if any. Skipped entirely when stdout isn't a
/// TTY or a CI environment is detected, so scripted/piped/CI output is never
/// polluted.
pub fn maybe_print_update_notice() {
    if !should_show_notice() || UPDATE_APPLIED.load(Ordering::Relaxed) {
        return;
    }
    let Some(path) = cache_path() else { return };
    let Some(cache) = load_cache(&path) else {
        return;
    };
    let Some(latest) = parse_version(&cache.latest_version) else {
        return;
    };
    if latest <= current_version() {
        return;
    }
    let _ = writeln!(
        std::io::stderr(),
        "\nA new gddy version is available: {} -> {latest} — run `gddy update apply` to install it.",
        current_version(),
    );
}

fn should_show_notice() -> bool {
    use std::io::IsTerminal as _;
    if !std::io::stdout().is_terminal() {
        return false;
    }
    if std::env::var_os("CI").is_some() || std::env::var_os("GITHUB_ACTIONS").is_some() {
        return false;
    }
    true
}

fn current_version() -> semver::Version {
    semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("CARGO_PKG_VERSION is valid semver")
}

fn parse_version(v: &str) -> Option<semver::Version> {
    semver::Version::parse(v.trim_start_matches('v')).ok()
}

fn cache_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("gddy").join(CACHE_FILE_NAME))
}

fn load_cache(path: &Path) -> Option<UpdateCache> {
    let contents = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents).ok()
}

fn save_cache(path: &Path, cache: &UpdateCache) -> Result<(), CliCoreError> {
    let contents = serde_json::to_string_pretty(cache)
        .map_err(|e| CliCoreError::message(format!("failed to serialize update cache: {e}")))?;
    cli_engine::fs::write_string_atomic(path, &contents)
}

fn is_stale(cache: &Option<UpdateCache>) -> bool {
    let Some(cache) = cache else { return true };
    let Ok(checked_at) = chrono::DateTime::parse_from_rfc3339(&cache.checked_at) else {
        return true;
    };
    let age = chrono::Utc::now().signed_duration_since(checked_at);
    age.to_std().map(|age| age > CACHE_TTL).unwrap_or(true)
}

fn http_client() -> Result<reqwest::Client, CliCoreError> {
    reqwest::Client::builder()
        .user_agent(concat!("gddy/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| CliCoreError::message(format!("failed to build HTTP client: {e}")))
}

async fn fetch_latest_release(
    client: &reqwest::Client,
    timeout: Duration,
) -> Result<GithubRelease, CliCoreError> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let resp = client
        .get(&url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| CliCoreError::message(format!("failed to check for updates: {e}")))?;
    if !resp.status().is_success() {
        return Err(CliCoreError::message(format!(
            "GitHub API returned {} while checking for updates",
            resp.status()
        )));
    }
    resp.json::<GithubRelease>()
        .await
        .map_err(|e| CliCoreError::message(format!("failed to parse GitHub release response: {e}")))
}

async fn refresh_cache(
    client: &reqwest::Client,
    timeout: Duration,
) -> Result<UpdateCache, CliCoreError> {
    let release = fetch_latest_release(client, timeout).await?;
    let version = parse_version(&release.tag_name).ok_or_else(|| {
        CliCoreError::message(format!(
            "unexpected tag format from GitHub: {}",
            release.tag_name
        ))
    })?;
    let cache = UpdateCache {
        checked_at: chrono::Utc::now().to_rfc3339(),
        latest_version: version.to_string(),
        tag_name: release.tag_name,
    };
    // Caching is advisory (only the passive notice depends on it) — a write
    // failure (e.g. read-only config dir) shouldn't fail the actual check.
    if let Some(path) = cache_path() {
        let _ = save_cache(&path, &cache);
    }
    Ok(cache)
}

async fn download(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, CliCoreError> {
    let resp = client
        .get(url)
        .timeout(FOREGROUND_TIMEOUT)
        .send()
        .await
        .map_err(|e| CliCoreError::message(format!("failed to download {url}: {e}")))?;
    if !resp.status().is_success() {
        return Err(CliCoreError::message(format!(
            "failed to download {url}: HTTP {}",
            resp.status()
        )));
    }
    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| CliCoreError::message(format!("failed to read response body from {url}: {e}")))
}

async fn download_text(client: &reqwest::Client, url: &str) -> Result<String, CliCoreError> {
    let bytes = download(client, url).await?;
    String::from_utf8(bytes).map_err(|e| {
        CliCoreError::message(format!("checksums file at {url} is not valid UTF-8: {e}"))
    })
}

/// Finds the expected checksum for `asset` in a `sha256sum`/`shasum`-style
/// checksums file (`<hex>  <filename>` per line), matching the filename
/// exactly — the same logic `install.sh` uses via `awk '$2 == f'`.
fn expected_checksum<'a>(checksums_text: &'a str, asset: &str) -> Option<&'a str> {
    checksums_text.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?;
        (name == asset).then_some(hash)
    })
}

fn verify_checksum(
    checksums_text: &str,
    asset: &str,
    archive_bytes: &[u8],
) -> Result<(), CliCoreError> {
    let expected = expected_checksum(checksums_text, asset).ok_or_else(|| {
        CliCoreError::message(format!("checksum for {asset} not found in checksums file"))
    })?;
    let mut hasher = Sha256::new();
    hasher.update(archive_bytes);
    let actual = format!("{:x}", hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(CliCoreError::message(format!(
            "checksum mismatch for {asset}: expected {expected}, got {actual}"
        )));
    }
    Ok(())
}

fn target_triple() -> Result<&'static str, CliCoreError> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        (os, arch) => Err(CliCoreError::message(format!(
            "unsupported platform for self-update: {os}-{arch}; download manually from \
             https://github.com/{REPO}/releases"
        ))),
    }
}

fn asset_name(target: &str) -> String {
    if target.contains("windows") {
        format!("gddy-{target}.zip")
    } else {
        format!("gddy-{target}.tar.gz")
    }
}

fn bin_name() -> &'static str {
    if cfg!(windows) { "gddy.exe" } else { "gddy" }
}

fn extract_tar_gz(archive_bytes: &[u8], bin_name: &str) -> Result<Vec<u8>, CliCoreError> {
    use std::io::Read as _;
    let decoder = flate2::read::GzDecoder::new(archive_bytes);
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|e| CliCoreError::message(format!("failed to read tar archive: {e}")))?;
    for entry in entries {
        let mut entry =
            entry.map_err(|e| CliCoreError::message(format!("failed to read tar entry: {e}")))?;
        let path = entry
            .path()
            .map_err(|e| CliCoreError::message(format!("failed to read tar entry path: {e}")))?;
        if path.file_name().and_then(|n| n.to_str()) == Some(bin_name) {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).map_err(|e| {
                CliCoreError::message(format!("failed to read {bin_name} from archive: {e}"))
            })?;
            return Ok(buf);
        }
    }
    Err(CliCoreError::message(format!(
        "{bin_name} not found in archive"
    )))
}

fn extract_zip(archive_bytes: &[u8], bin_name: &str) -> Result<Vec<u8>, CliCoreError> {
    use std::io::Read as _;
    let reader = std::io::Cursor::new(archive_bytes);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| CliCoreError::message(format!("failed to read zip archive: {e}")))?;
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| CliCoreError::message(format!("failed to read zip entry: {e}")))?;
        let matches = Path::new(file.name()).file_name().and_then(|n| n.to_str()) == Some(bin_name);
        if matches {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).map_err(|e| {
                CliCoreError::message(format!("failed to read {bin_name} from archive: {e}"))
            })?;
            return Ok(buf);
        }
    }
    Err(CliCoreError::message(format!(
        "{bin_name} not found in archive"
    )))
}

fn write_temp_binary(bytes: &[u8]) -> Result<PathBuf, CliCoreError> {
    let path = std::env::temp_dir().join(format!("gddy-update-{}", uuid::Uuid::new_v4()));
    std::fs::write(&path, bytes)
        .map_err(|e| CliCoreError::message(format!("failed to write temporary binary: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).map_err(|e| {
            CliCoreError::message(format!("failed to set executable permission: {e}"))
        })?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tags_with_and_without_v_prefix() {
        assert_eq!(
            parse_version("v1.2.3").expect("valid semver").to_string(),
            "1.2.3"
        );
        assert_eq!(
            parse_version("1.2.3").expect("valid semver").to_string(),
            "1.2.3"
        );
        assert!(parse_version("not-a-version").is_none());
    }

    #[test]
    fn semver_compares_correctly_not_lexicographically() {
        let v0_9 = parse_version("0.9.0").expect("valid semver");
        let v0_10 = parse_version("0.10.0").expect("valid semver");
        assert!(v0_10 > v0_9);
    }

    #[test]
    fn target_triple_maps_known_platforms() {
        assert_eq!(
            asset_name("x86_64-unknown-linux-gnu"),
            "gddy-x86_64-unknown-linux-gnu.tar.gz"
        );
        assert_eq!(
            asset_name("x86_64-pc-windows-msvc"),
            "gddy-x86_64-pc-windows-msvc.zip"
        );
    }

    #[test]
    fn expected_checksum_matches_exact_filename_only() {
        let checksums = "\
abc123  gddy-x86_64-unknown-linux-gnu.tar.gz
def456  gddy-aarch64-unknown-linux-gnu.tar.gz
";
        assert_eq!(
            expected_checksum(checksums, "gddy-x86_64-unknown-linux-gnu.tar.gz"),
            Some("abc123")
        );
        assert_eq!(
            expected_checksum(checksums, "gddy-x86_64-unknown-linux-gnu.tar.gz.evil"),
            None
        );
    }

    #[test]
    fn verify_checksum_accepts_matching_and_rejects_mismatched() {
        let data = b"hello world";
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = format!("{:x}", hasher.finalize());
        let checksums = format!("{hash}  asset.tar.gz\n");

        assert!(verify_checksum(&checksums, "asset.tar.gz", data).is_ok());
        assert!(verify_checksum(&checksums, "asset.tar.gz", b"tampered").is_err());
        assert!(verify_checksum(&checksums, "missing.tar.gz", data).is_err());
    }

    #[test]
    fn cache_round_trips_through_disk() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join(CACHE_FILE_NAME);

        let cache = UpdateCache {
            checked_at: chrono::Utc::now().to_rfc3339(),
            latest_version: "9.9.9".to_owned(),
            tag_name: "v9.9.9".to_owned(),
        };
        save_cache(&path, &cache).expect("save");

        let loaded = load_cache(&path).expect("load");
        assert_eq!(loaded.latest_version, "9.9.9");
    }

    #[test]
    fn missing_cache_file_is_treated_as_stale_not_an_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("does-not-exist.json");
        assert!(load_cache(&path).is_none());
        assert!(is_stale(&None));
    }

    #[test]
    fn fresh_cache_is_not_stale_and_old_cache_is() {
        let fresh = Some(UpdateCache {
            checked_at: chrono::Utc::now().to_rfc3339(),
            latest_version: "1.0.0".to_owned(),
            tag_name: "v1.0.0".to_owned(),
        });
        assert!(!is_stale(&fresh));

        let old = Some(UpdateCache {
            checked_at: (chrono::Utc::now() - chrono::Duration::hours(48)).to_rfc3339(),
            latest_version: "1.0.0".to_owned(),
            tag_name: "v1.0.0".to_owned(),
        });
        assert!(is_stale(&old));
    }

    #[test]
    fn extract_tar_gz_finds_binary_by_exact_name() {
        use std::io::Write as _;

        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            let data = b"binary-contents";
            let mut header = tar::Header::new_gnu();
            header.set_path("gddy").expect("set path");
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append(&header, &data[..]).expect("append");
            builder.finish().expect("finish");
        }
        let mut gz_bytes = Vec::new();
        {
            let mut encoder =
                flate2::write::GzEncoder::new(&mut gz_bytes, flate2::Compression::default());
            encoder.write_all(&tar_bytes).expect("write");
            encoder.finish().expect("finish");
        }

        let extracted = extract_tar_gz(&gz_bytes, "gddy").expect("extract");
        assert_eq!(extracted, b"binary-contents");

        assert!(extract_tar_gz(&gz_bytes, "not-gddy").is_err());
    }

    #[test]
    fn extract_zip_finds_binary_by_exact_name() {
        let mut zip_bytes = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_bytes));
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            writer.start_file("gddy.exe", options).expect("start file");
            std::io::Write::write_all(&mut writer, b"binary-contents").expect("write");
            writer.finish().expect("finish");
        }

        let extracted = extract_zip(&zip_bytes, "gddy.exe").expect("extract");
        assert_eq!(extracted, b"binary-contents");

        assert!(extract_zip(&zip_bytes, "not-gddy.exe").is_err());
    }
}
