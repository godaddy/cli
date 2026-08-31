//! Refreshes the vendored Node.js Hosting OpenAPI spec
//! (`schemas/openapi/hosting-nodejs-public-v1.yaml`) that both the embedded
//! API catalog and the `gddy hosting nodejs` commands' `--schema` metadata
//! read. The hosting API publishes its spec on prod only (OTE has no spec
//! endpoint): `https://api.godaddy.com/v1/hosting/nodejs/openapi.yaml`.
//!
//! Overrides (matching the former `rust/scripts/regenerate-hosting-spec.sh`):
//!   `HOSTING_SPEC_URL`  — download from this URL instead (local Katana/dev
//!                          only; not OTE)
//!   `HOSTING_SPEC_PATH` — copy from a local file (e.g. a hosting-web-apps
//!                          checkout)

use std::{path::Path, time::Duration};

use anyhow::{Context, Result};

const DEFAULT_URL: &str = "https://api.godaddy.com/v1/hosting/nodejs/openapi.yaml";
const USER_AGENT: &str = "godaddy-cli-api-catalog-generator";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Refreshes `out_path` from the configured source. Set `SKIP_HOSTING_REFRESH`
/// to leave `out_path` untouched (useful for local iteration without a
/// network dependency).
pub(crate) fn refresh(out_path: &Path) -> Result<()> {
    if std::env::var("SKIP_HOSTING_REFRESH").is_ok() {
        eprintln!(
            "SKIP_HOSTING_REFRESH set — leaving {} untouched",
            out_path.display()
        );
        return Ok(());
    }

    let body = if let Ok(path) = std::env::var("HOSTING_SPEC_PATH") {
        eprintln!("Copying hosting-nodejs spec from HOSTING_SPEC_PATH={path}");
        std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read HOSTING_SPEC_PATH={path}"))?
    } else {
        let url = std::env::var("HOSTING_SPEC_URL").unwrap_or_else(|_| DEFAULT_URL.to_owned());
        eprintln!("Downloading hosting-nodejs spec from {url}");
        let client = reqwest::blocking::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("failed to build http client")?;
        client
            .get(&url)
            .send()
            .with_context(|| format!("failed to fetch hosting-nodejs spec from {url}"))?
            .error_for_status()
            .with_context(|| {
                format!("hosting-nodejs spec fetch from {url} returned an error status")
            })?
            .text()
            .context("failed to read hosting-nodejs spec response body")?
    };

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(out_path, body)
        .with_context(|| format!("failed to write {}", out_path.display()))?;
    eprintln!("   wrote {}", out_path.display());
    Ok(())
}
