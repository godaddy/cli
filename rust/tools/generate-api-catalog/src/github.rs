use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use base64::Engine;
use serde::Deserialize;
use serde_json::Value;

use crate::manifest::{CatalogSourceManifest, RemoteCatalogSource};

const GITHUB_ORG: &str = "gdcorp-platform";
const GITHUB_API_BASE: &str = "https://api.github.com";
const GITHUB_PAGE_SIZE: u32 = 100;

// ---------------------------------------------------------------------------
// GitHub discovery
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct GithubRepo {
    name: String,
    clone_url: String,
    archived: bool,
    disabled: bool,
    private: bool,
}

pub(crate) struct SpecSource {
    pub(crate) domain: String,
    pub(crate) repo_name: String,
    pub(crate) spec_file: PathBuf,
    pub(crate) spec_version: String,
    pub(crate) graphql_only: bool,
}

fn github_client() -> Result<reqwest::blocking::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::ACCEPT,
        "application/vnd.github+json"
            .parse()
            .expect("valid header value"),
    );
    headers.insert(
        reqwest::header::USER_AGENT,
        "godaddy-cli-api-catalog-generator"
            .parse()
            .expect("valid header value"),
    );
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        let token = token.trim().to_owned();
        if !token.is_empty() {
            let auth_value = format!("Bearer {token}").parse().context("invalid token")?;
            headers.insert(reqwest::header::AUTHORIZATION, auth_value);
        }
    }
    reqwest::blocking::Client::builder()
        .default_headers(headers)
        .build()
        .context("failed to build HTTP client")
}

fn list_repos_for_owner_path(
    client: &reqwest::blocking::Client,
    owner_path: &str,
) -> Result<Vec<GithubRepo>> {
    let mut repos = Vec::new();
    let mut page = 1u32;
    loop {
        let url = format!(
            "{GITHUB_API_BASE}/{owner_path}/{GITHUB_ORG}/repos?per_page={GITHUB_PAGE_SIZE}&page={page}&type=public&sort=full_name&direction=asc"
        );
        let resp = client
            .get(&url)
            .send()
            .context("GitHub API request failed")?;
        let status = resp.status();
        if status == 404 {
            return Ok(Vec::new());
        }
        if !status.is_success() {
            bail!("GitHub API returned {} for {}", status, url);
        }
        let batch: Vec<GithubRepo> = resp.json().context("failed to parse GitHub repos")?;
        let is_last = batch.len() < GITHUB_PAGE_SIZE as usize;
        repos.extend(batch);
        if is_last {
            break;
        }
        page += 1;
    }
    Ok(repos)
}

fn list_org_repos(client: &reqwest::blocking::Client) -> Vec<GithubRepo> {
    match list_repos_for_owner_path(client, "orgs") {
        Ok(repos) if !repos.is_empty() => return repos,
        Ok(_) => {}
        Err(e) => eprintln!("WARNING: GitHub orgs API failed: {e}"),
    }
    match list_repos_for_owner_path(client, "users") {
        Ok(repos) => repos,
        Err(e) => {
            eprintln!("WARNING: GitHub users API also failed: {e}");
            Vec::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Repo/spec discovery helpers
// ---------------------------------------------------------------------------

fn parse_version_dir(name: &str) -> Option<Vec<u32>> {
    if !name.starts_with('v') {
        return None;
    }
    name[1..]
        .split('.')
        .map(|s| s.parse::<u32>().ok())
        .collect::<Option<Vec<_>>>()
}

fn find_latest_spec_file(repo_dir: &Path) -> Option<(String, PathBuf, bool)> {
    let mut candidates: Vec<(Vec<u32>, String)> = std::fs::read_dir(repo_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            parse_version_dir(&name).map(|v| (v, name))
        })
        .collect();
    candidates.sort_by(|(a, _), (b, _)| a.cmp(b));

    for (_, version) in candidates.iter().rev() {
        for name in &["openapi.yaml", "openapi.yml", "openapi.json"] {
            let p = repo_dir.join(version).join("schemas").join(name);
            if p.exists() {
                return Some((version.clone(), p, false));
            }
        }
        for name in &["graphql/schema.graphql", "schema.graphql"] {
            let p = repo_dir.join(version).join("schemas").join(name);
            if p.exists() {
                return Some((version.clone(), p, true));
            }
        }
    }
    None
}

fn git_auth_config(token: Option<&str>) -> HashMap<String, String> {
    let mut config = HashMap::from([(
        "url.https://github.com/.insteadOf".to_owned(),
        "git@github.com:".to_owned(),
    )]);
    if let Some(token) = token.map(str::trim).filter(|token| !token.is_empty()) {
        let credentials =
            base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"));
        config.insert(
            "http.https://github.com/.extraHeader".to_owned(),
            format!("AUTHORIZATION: basic {credentials}"),
        );
    }
    config
}

fn git_command() -> Command {
    let token = std::env::var("GITHUB_TOKEN").ok();
    let config = git_auth_config(token.as_deref());
    let mut command = Command::new("git");
    command.env("GIT_CONFIG_COUNT", config.len().to_string());
    for (index, (key, value)) in config.into_iter().enumerate() {
        command.env(format!("GIT_CONFIG_KEY_{index}"), key);
        command.env(format!("GIT_CONFIG_VALUE_{index}"), value);
    }
    command
}

fn git_run(args: &[&str]) -> Result<()> {
    let status = git_command()
        .args(args)
        .status()
        .context("failed to run git")?;
    if !status.success() {
        bail!("git {} exited with {}", args.join(" "), status);
    }
    Ok(())
}

fn clone_repo(clone_url: &str, target: &Path, git_ref: Option<&str>) -> Result<()> {
    git_run(&[
        "clone",
        "--depth",
        "1",
        "--recurse-submodules",
        "--shallow-submodules",
        "--quiet",
        clone_url,
        &target.to_string_lossy(),
    ])?;
    if let Some(r) = git_ref {
        git_run(&[
            "-C",
            &target.to_string_lossy(),
            "fetch",
            "--depth",
            "1",
            "origin",
            r,
        ])?;
        git_run(&[
            "-C",
            &target.to_string_lossy(),
            "checkout",
            "--quiet",
            "FETCH_HEAD",
        ])?;
    }
    Ok(())
}

fn parse_repo_overrides() -> Option<Vec<String>> {
    let raw = std::env::var("API_CATALOG_REPOS").ok()?;
    let repos: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    if repos.is_empty() { None } else { Some(repos) }
}

fn parse_repo_ref_overrides() -> HashMap<String, String> {
    let raw = match std::env::var("API_CATALOG_REPO_REFS") {
        Ok(v) => v,
        Err(_) => return HashMap::new(),
    };
    let mut map = HashMap::new();
    for entry in raw.split(',') {
        let entry = entry.trim();
        if let Some(eq) = entry.find('=') {
            let repo = entry[..eq].trim().to_owned();
            let git_ref = entry[eq + 1..].trim().to_owned();
            if !repo.is_empty() && !git_ref.is_empty() {
                map.insert(repo, git_ref);
            }
        }
    }
    map
}

pub(crate) fn discover_spec_sources(
    manifest: &CatalogSourceManifest,
) -> Result<(Vec<SpecSource>, PathBuf)> {
    let client = github_client()?;
    let all_repos = list_org_repos(&client);
    let repo_map: HashMap<&str, &GithubRepo> =
        all_repos.iter().map(|r| (r.name.as_str(), r)).collect();

    let overrides = parse_repo_overrides();
    let ref_overrides = parse_repo_ref_overrides();
    let selected: Vec<&RemoteCatalogSource> = match overrides {
        Some(repositories) => {
            let selected: Vec<&RemoteCatalogSource> = repositories
                .iter()
                .map(|repository| {
                    manifest
                        .remote
                        .iter()
                        .find(|source| source.repository == *repository)
                        .with_context(|| {
                            format!(
                                "API_CATALOG_REPOS contains '{repository}', which is not declared in api-catalog-sources.json"
                            )
                        })
                })
                .collect::<Result<_>>()?;
            selected
        }
        None => manifest.remote.iter().collect(),
    };

    // Create temp directory
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let tmpdir = std::env::temp_dir().join(format!("godaddy-api-catalog-{timestamp}"));
    std::fs::create_dir_all(&tmpdir).context("failed to create temp dir")?;

    // Clone common-types-specification
    let common_types_dir = tmpdir.join("__common-types");
    let ct_url = format!("https://github.com/{GITHUB_ORG}/common-types-specification.git");
    clone_repo(&ct_url, &common_types_dir, None)
        .context("failed to clone declared common-types specification source")?;

    let mut sources = Vec::new();
    let mut used_domains = HashSet::new();

    for source in selected {
        let repo_name = &source.repository;
        let clone_url = if let Some(repo) = repo_map.get(repo_name.as_str()) {
            if repo.archived || repo.disabled || repo.private {
                bail!("catalog source repository '{repo_name}' is unavailable");
            }
            repo.clone_url.clone()
        } else {
            format!("https://github.com/{GITHUB_ORG}/{repo_name}.git")
        };
        let repo_dir = tmpdir.join(repo_name);
        let git_ref = ref_overrides.get(repo_name.as_str()).map(String::as_str);

        clone_repo(&clone_url, &repo_dir, git_ref)
            .with_context(|| format!("failed to clone declared catalog source '{repo_name}'"))?;

        let (version, spec_file, graphql_only) = find_latest_spec_file(&repo_dir)
            .with_context(|| format!("catalog source '{repo_name}' has no versioned spec file"))?;

        let domain = source.domain.clone();
        if !used_domains.insert(domain.clone()) {
            eprintln!("WARNING: duplicate domain '{domain}' from '{repo_name}' — skipping");
            continue;
        }

        sources.push(SpecSource {
            domain,
            repo_name: repo_name.clone(),
            spec_file,
            spec_version: version,
            graphql_only,
        });
    }

    Ok((sources, tmpdir))
}

pub(crate) fn local_spec_sources(manifest: &CatalogSourceManifest) -> Result<Vec<SpecSource>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let rust_dir = manifest_dir.join("../..");
    let mut sources = Vec::new();
    for source in &manifest.local {
        let spec_file = rust_dir.join(&source.path);
        if !spec_file.exists() {
            bail!("local catalog spec {} not found", spec_file.display());
        }
        let src = std::fs::read_to_string(&spec_file)
            .with_context(|| format!("failed to read {}", spec_file.display()))?;
        let spec = parse_yaml_or_json(&src, &spec_file)?;
        let raw_version = spec
            .get("info")
            .and_then(|i| i.get("version"))
            .and_then(|v| v.as_str())
            .unwrap_or("1.0.0");
        let spec_version = if raw_version.starts_with('v') {
            raw_version.to_owned()
        } else {
            format!("v{raw_version}")
        };
        sources.push(SpecSource {
            domain: source.domain.clone(),
            repo_name: format!("local:{}", source.domain),
            spec_file,
            spec_version,
            graphql_only: false,
        });
    }
    Ok(sources)
}

// ---------------------------------------------------------------------------
// YAML/JSON parsing
// ---------------------------------------------------------------------------

pub(crate) fn parse_yaml_or_json(src: &str, path: &Path) -> Result<Value> {
    // serde_yaml handles both YAML and JSON
    let first_err = match serde_yaml::from_str::<Value>(src) {
        Ok(v) => return Ok(v),
        Err(e) => e,
    };
    // Some spec files authored on macOS use YAML double-quoted strings
    // containing `\.` (a regex dot-escape) which is not a valid YAML 1.2
    // escape sequence.  Retry after normalising `\.` → `\\.` inside
    // double-quoted scalars so the parser accepts it.
    let fixed = fix_yaml_invalid_dot_escapes(src);
    serde_yaml::from_str::<Value>(&fixed).with_context(|| {
        format!(
            "failed to parse {} (original error: {first_err})",
            path.display()
        )
    })
}

/// Replace `\.` with `\\.` inside YAML double-quoted scalars.
/// This handles the common case where regex patterns written for
/// case-insensitive filesystems contain bare `\.` in `"..."` strings.
fn fix_yaml_invalid_dot_escapes(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut in_dq = false;
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if !in_dq {
            out.push(c);
            if c == '"' {
                in_dq = true;
            }
        } else {
            match c {
                '"' => {
                    out.push(c);
                    in_dq = false;
                }
                '\\' => {
                    if chars.peek() == Some(&'.') {
                        // `\.` → `\\.` (escape the backslash so the dot is literal)
                        out.push_str("\\\\.");
                        chars.next();
                    } else if chars.peek() == Some(&'"') {
                        // `\"` — consume both so the closing quote doesn't end the scalar
                        out.push_str("\\\"");
                        chars.next();
                    } else {
                        out.push(c);
                    }
                }
                _ => out.push(c),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_auth_config_rewrites_ssh_submodules_without_exposing_the_token() {
        let config = git_auth_config(Some("test-token"));

        assert_eq!(
            config.get("url.https://github.com/.insteadOf"),
            Some(&"git@github.com:".to_owned())
        );
        let header = config
            .get("http.https://github.com/.extraHeader")
            .expect("authorization header");
        assert!(header.starts_with("AUTHORIZATION: basic "));
        assert!(!header.contains("test-token"));
    }
}
