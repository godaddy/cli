use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use base64::Engine;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const GITHUB_ORG: &str = "gdcorp-platform";
const GITHUB_API_BASE: &str = "https://api.github.com";
const GITHUB_PAGE_SIZE: u32 = 100;
const SOURCE_MANIFEST_JSON: &str = include_str!("../../../api-catalog-sources.json");

const HTTP_METHODS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "options", "head", "trace",
];

// ---------------------------------------------------------------------------
// Output catalog types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct CatalogGraphqlArgument {
    name: String,
    #[serde(rename = "type")]
    arg_type: String,
    required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(rename = "defaultValue", skip_serializing_if = "Option::is_none")]
    default_value: Option<String>,
}

#[derive(Debug, Serialize)]
struct CatalogGraphqlOperation {
    name: String,
    kind: String,
    #[serde(rename = "returnType")]
    return_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    deprecated: bool,
    #[serde(rename = "deprecationReason", skip_serializing_if = "Option::is_none")]
    deprecation_reason: Option<String>,
    args: Vec<CatalogGraphqlArgument>,
}

#[derive(Debug, Serialize)]
struct CatalogGraphqlSchema {
    #[serde(rename = "schemaRef")]
    schema_ref: String,
    #[serde(rename = "operationCount")]
    operation_count: usize,
    operations: Vec<CatalogGraphqlOperation>,
}

#[derive(Debug, Serialize)]
struct CatalogParameter {
    name: String,
    #[serde(rename = "in")]
    location: String,
    required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<Value>,
}

#[derive(Debug, Serialize)]
struct CatalogRequestBody {
    required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(rename = "contentType")]
    content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<Value>,
}

#[derive(Debug, Serialize)]
struct CatalogResponse {
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<Value>,
}

#[derive(Debug, Serialize)]
struct CatalogEndpoint {
    #[serde(rename = "operationId")]
    operation_id: String,
    method: String,
    path: String,
    summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<Vec<CatalogParameter>>,
    #[serde(rename = "requestBody", skip_serializing_if = "Option::is_none")]
    request_body: Option<CatalogRequestBody>,
    responses: HashMap<String, CatalogResponse>,
    scopes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    graphql: Option<CatalogGraphqlSchema>,
}

#[derive(Debug, Serialize)]
struct CatalogDomain {
    name: String,
    title: String,
    description: String,
    version: String,
    #[serde(rename = "baseUrl")]
    base_url: String,
    endpoints: Vec<CatalogEndpoint>,
}

#[derive(Debug, Serialize)]
struct ManifestEntry {
    file: String,
    title: String,
    #[serde(rename = "endpointCount")]
    endpoint_count: usize,
}

#[derive(Debug, Serialize)]
struct CatalogManifest {
    generated: String,
    domains: HashMap<String, ManifestEntry>,
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RemoteCatalogSource {
    domain: String,
    repository: String,
}

#[derive(Debug, Deserialize)]
struct LocalCatalogSource {
    domain: String,
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyTypescriptParity {
    status: String,
    shared_domain_count: usize,
    rust_only_domains: Vec<String>,
    rationale: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogSourceManifest {
    version: u32,
    remote: Vec<RemoteCatalogSource>,
    local: Vec<LocalCatalogSource>,
    legacy_typescript: LegacyTypescriptParity,
}

impl CatalogSourceManifest {
    fn expected_domains(&self) -> Vec<String> {
        let mut domains: Vec<String> = self
            .remote
            .iter()
            .map(|source| source.domain.clone())
            .chain(self.local.iter().map(|source| source.domain.clone()))
            .collect();
        domains.sort();
        domains
    }
}

#[derive(Debug, Deserialize)]
struct GithubRepo {
    name: String,
    clone_url: String,
    archived: bool,
    disabled: bool,
    private: bool,
}

struct SpecSource {
    domain: String,
    repo_name: String,
    spec_file: PathBuf,
    spec_version: String,
    graphql_only: bool,
}

fn load_source_manifest() -> Result<CatalogSourceManifest> {
    let manifest: CatalogSourceManifest = serde_json::from_str(SOURCE_MANIFEST_JSON)
        .context("failed to parse api-catalog-sources.json")?;

    if manifest.version != 1 {
        bail!(
            "unsupported api-catalog-sources.json version {}",
            manifest.version
        );
    }
    if manifest.legacy_typescript.status != "retired-on-rust-port" {
        bail!("legacyTypescript.status must document the Rust-port retirement");
    }
    if manifest.legacy_typescript.shared_domain_count != manifest.remote.len() {
        bail!(
            "legacyTypescript.sharedDomainCount must match the {} remote domains",
            manifest.remote.len()
        );
    }
    if manifest.legacy_typescript.rationale.trim().is_empty() {
        bail!("legacyTypescript.rationale must document the catalog difference");
    }

    let expected = manifest.expected_domains();
    let unique: HashSet<&str> = expected.iter().map(String::as_str).collect();
    if unique.len() != expected.len() {
        bail!("api-catalog-sources.json contains duplicate domain names");
    }

    let mut local_domains: Vec<&str> = manifest
        .local
        .iter()
        .map(|source| source.domain.as_str())
        .collect();
    local_domains.sort();
    let mut rust_only_domains: Vec<&str> = manifest
        .legacy_typescript
        .rust_only_domains
        .iter()
        .map(String::as_str)
        .collect();
    rust_only_domains.sort();
    if rust_only_domains != local_domains {
        bail!("legacyTypescript.rustOnlyDomains must match the local source domains");
    }

    Ok(manifest)
}

// ---------------------------------------------------------------------------
// GitHub discovery
// ---------------------------------------------------------------------------

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

fn discover_spec_sources(manifest: &CatalogSourceManifest) -> Result<(Vec<SpecSource>, PathBuf)> {
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

fn local_spec_sources(manifest: &CatalogSourceManifest) -> Result<Vec<SpecSource>> {
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

fn parse_yaml_or_json(src: &str, path: &Path) -> Result<Value> {
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

// ---------------------------------------------------------------------------
// $ref resolution
// ---------------------------------------------------------------------------

fn resolve_local_ref<'a>(root: &'a Value, pointer: &str) -> Option<&'a Value> {
    let path = pointer.strip_prefix("#/")?;
    let mut cur = root;
    for seg in path.split('/') {
        let seg = seg.replace("~1", "/").replace("~0", "~");
        cur = cur.get(&seg)?;
    }
    Some(cur)
}

/// Returns the `$defs` key to use when lifting a `$ref` string, or `None` if the ref
/// should be inlined as before.
///
/// Lifted: external file/URL refs, and `#/components/schemas/` / `#/definitions/` local refs.
/// Inlined: all other local refs (e.g. `#/paths/...`).
fn should_lift_to_defs(ref_str: &str) -> Option<String> {
    // External file or URL: lift using a key derived from the file name
    if !ref_str.starts_with('#') {
        return Some(derive_defs_key_for_path(ref_str));
    }
    // Local schema component refs
    for prefix in &["#/components/schemas/", "#/definitions/"] {
        if let Some(rest) = ref_str.strip_prefix(prefix) {
            return Some(rest.to_owned());
        }
    }
    None
}

/// Escape a string for use as a single JSON Pointer segment (RFC 6901).
/// `~` → `~0`, `/` → `~1`.
fn json_pointer_escape(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

/// Derive a stable `$defs` key from an external ref path or URL.
/// Examples:
///   `./models/Order.yaml`  → `"Order"`
///   `https://.../error.json` → `"error"`
fn derive_defs_key_for_path(ref_str: &str) -> String {
    // External refs with a component-schema fragment (e.g. `./common.yaml#/components/schemas/Foo`)
    // use the component name as the key, consistent with pure local `#/components/schemas/` refs
    // and avoiding collisions when the same file is referenced for different components.
    if let Some(frag_idx) = ref_str.find('#') {
        let frag = &ref_str[frag_idx..];
        for prefix in &["#/components/schemas/", "#/definitions/"] {
            if let Some(rest) = frag.strip_prefix(prefix) {
                let name = rest.split('/').next().unwrap_or(rest);
                return sanitize_defs_key(name);
            }
        }
    }
    let file_part = ref_str.split('#').next().unwrap_or(ref_str);
    let last_seg = file_part.rsplit(['/', '\\']).next().unwrap_or(file_part);
    let stem = match last_seg.rfind('.') {
        Some(pos) => &last_seg[..pos],
        None => last_seg,
    };
    sanitize_defs_key(stem)
}

fn sanitize_defs_key(s: &str) -> String {
    let sanitized: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    sanitized.trim_matches('_').to_owned()
}

fn dereference(
    root: &Value,
    current: Value,
    spec_dir: &Path,
    common_types_dir: Option<&Path>,
    defs: &mut IndexMap<String, Value>,
    depth: usize,
) -> Value {
    if depth > 64 {
        return current;
    }
    match current {
        Value::Object(mut map) => {
            if let Some(Value::String(ref_str)) = map.get("$ref").cloned() {
                if let Some(key) = should_lift_to_defs(&ref_str) {
                    // Already registered — return the local pointer immediately (dedup)
                    if defs.contains_key(&key) {
                        return serde_json::json!({"$ref": format!("#/$defs/{}", json_pointer_escape(&key))});
                    }
                    // Insert null placeholder before resolving (circular ref guard)
                    defs.insert(key.clone(), Value::Null);
                    let resolved_opt = if ref_str.starts_with('#') {
                        // Local component ref: resolve from this spec's root
                        resolve_local_ref(root, &ref_str).cloned().map(|v| {
                            dereference(root, v, spec_dir, common_types_dir, defs, depth + 1)
                        })
                    } else {
                        // External file/URL ref
                        resolve_ref(root, &ref_str, spec_dir, common_types_dir, defs, depth + 1)
                    };
                    if let Some(content) = resolved_opt {
                        defs.insert(key.clone(), content);
                        return serde_json::json!({"$ref": format!("#/$defs/{}", json_pointer_escape(&key))});
                    }
                    // Resolution failed — remove placeholder and preserve original $ref
                    // so collect_unresolved_refs() will catch it.
                    defs.shift_remove(&key);
                    return Value::Object(map);
                }
                // Non-liftable local ref (e.g. #/paths/...) — inline as before
                let resolved =
                    resolve_ref(root, &ref_str, spec_dir, common_types_dir, defs, depth + 1);
                return resolved
                    .map(|v| dereference(root, v, spec_dir, common_types_dir, defs, depth + 1))
                    .unwrap_or(Value::Object(map));
            }
            // Recurse into all values
            for v in map.values_mut() {
                *v = dereference(root, v.take(), spec_dir, common_types_dir, defs, depth + 1);
            }
            Value::Object(map)
        }
        Value::Array(mut arr) => {
            for v in &mut arr {
                *v = dereference(root, v.take(), spec_dir, common_types_dir, defs, depth + 1);
            }
            Value::Array(arr)
        }
        other => other,
    }
}

fn resolve_ref(
    root: &Value,
    ref_str: &str,
    spec_dir: &Path,
    common_types_dir: Option<&Path>,
    defs: &mut IndexMap<String, Value>,
    depth: usize,
) -> Option<Value> {
    if ref_str.starts_with("#/") {
        return resolve_local_ref(root, ref_str).cloned();
    }

    if ref_str.starts_with("https://schemas.api.godaddy.com/") {
        let ct_dir = common_types_dir?;
        let url_path = ref_str
            .strip_prefix("https://schemas.api.godaddy.com")
            .unwrap_or("")
            .strip_prefix("/common-types")
            .unwrap_or(
                ref_str
                    .strip_prefix("https://schemas.api.godaddy.com")
                    .unwrap_or(""),
            );
        let local_path = ct_dir.join(url_path.trim_start_matches('/'));
        return load_external_ref(&local_path, common_types_dir, defs, depth);
    }

    // Relative file reference — strip fragment
    let (file_part, fragment) = match ref_str.find('#') {
        Some(i) => (&ref_str[..i], Some(&ref_str[i..])),
        None => (ref_str, None),
    };

    if file_part.is_empty() {
        // Same-file fragment reference
        return resolve_local_ref(root, fragment?).cloned();
    }

    let file_path = spec_dir.join(file_part);
    // Some repos store model files alongside the `schemas/` directory rather than
    // inside it (e.g. `v2/models/` instead of `v2/schemas/models/`).  Try the
    // parent of spec_dir as a fallback so both layouts resolve correctly.
    let external_root =
        load_external_ref(&file_path, common_types_dir, defs, depth).or_else(|| {
            spec_dir.parent().and_then(|parent| {
                load_external_ref(&parent.join(file_part), common_types_dir, defs, depth)
            })
        })?;

    if let Some(frag) = fragment
        && !frag.is_empty()
    {
        return resolve_local_ref(&external_root, frag).cloned();
    }
    Some(external_root)
}

fn load_external_ref(
    path: &Path,
    common_types_dir: Option<&Path>,
    defs: &mut IndexMap<String, Value>,
    depth: usize,
) -> Option<Value> {
    // Normalize path (resolve .. segments) so the common-types check below works on
    // paths like ./models/../common-types/v1/schemas/yaml/uuid.yaml
    let normalized = normalize_path(path);
    let resolved = if normalized.exists() {
        normalized.clone()
    } else if let Some(fallback) = resolve_common_types_path(&normalized, common_types_dir) {
        fallback
    } else if let Some(ci) = resolve_case_insensitive(&normalized) {
        // Spec files authored on case-insensitive filesystems (macOS) sometimes
        // have mismatched filename casing.  Accept the match but warn so the
        // spec repo can be fixed.
        eprintln!(
            "WARNING: case-insensitive match for {}: using {}",
            path.display(),
            ci.display()
        );
        ci
    } else {
        eprintln!(
            "WARNING: cannot read ref file {}: No such file or directory (os error 2)",
            path.display()
        );
        return None;
    };

    let src = std::fs::read_to_string(&resolved)
        .map_err(|e| eprintln!("WARNING: cannot read ref file {}: {e}", resolved.display()))
        .ok()?;
    let parsed = parse_yaml_or_json(&src, &resolved)
        .map_err(|e| eprintln!("WARNING: cannot parse ref file {}: {e}", resolved.display()))
        .ok()?;
    let dir = resolved.parent().unwrap_or(&resolved);
    Some(dereference(
        &parsed.clone(),
        parsed,
        dir,
        common_types_dir,
        defs,
        depth + 1,
    ))
}

/// Try to find `path` in its parent directory with a case-insensitive filename match.
/// Returns the first candidate whose lowercased name equals the lowercased target,
/// or `None` if no unique match is found.
fn resolve_case_insensitive(path: &Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    let target = path.file_name()?.to_string_lossy().to_lowercase();
    let mut matches: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().to_lowercase() == target)
        .map(|e| e.path())
        .collect();
    if matches.len() == 1 {
        matches.pop()
    } else {
        None
    }
}

/// Normalize a path by resolving `.` and `..` components without hitting the filesystem.
fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

/// When a path contains a `common-types` segment and does not exist, try to locate the
/// file in the standalone common-types-specification clone. Mirrors the TypeScript
/// `resolveCommonTypesFile` helper.
fn resolve_common_types_path(path: &Path, common_types_dir: Option<&Path>) -> Option<PathBuf> {
    let ct_dir = common_types_dir?;
    let path_str = path.to_string_lossy();
    let ct_marker = "common-types/";
    let idx = path_str.find(ct_marker)?;

    let rel = &path_str[idx + ct_marker.len()..];
    // 1. Try path as-is relative to the clone root
    let direct = ct_dir.join(rel);
    if direct.exists() {
        return Some(direct);
    }

    // 2. Try v1/schemas/{yaml,json}/<basename> (covering both extension variants)
    let basename = path.file_name()?;
    let basename_str = basename.to_string_lossy();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let (primary_sub, alt_sub, alt_ext) = if ext == "json" {
        ("json", "yaml", "yaml")
    } else {
        ("yaml", "json", "json")
    };

    let nested = ct_dir
        .join("v1")
        .join("schemas")
        .join(primary_sub)
        .join(basename_str.as_ref());
    if nested.exists() {
        return Some(nested);
    }
    let stem = path.file_stem()?.to_string_lossy();
    let alt_name = format!("{stem}.{alt_ext}");
    let alt = ct_dir
        .join("v1")
        .join("schemas")
        .join(alt_sub)
        .join(&alt_name);
    if alt.exists() {
        return Some(alt);
    }

    None
}

/// Walk a dereferenced value and collect any remaining external `$ref` strings.
/// External refs are anything that is not a local JSON pointer (`#/...`).
fn collect_unresolved_refs(value: &Value) -> Vec<String> {
    let mut found = Vec::new();
    collect_unresolved_refs_inner(value, &mut found);
    found.sort();
    found.dedup();
    found
}

fn collect_unresolved_refs_inner(value: &Value, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get("$ref")
                && !r.starts_with('#')
            {
                found.push(r.clone());
            }
            for v in map.values() {
                collect_unresolved_refs_inner(v, found);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                collect_unresolved_refs_inner(v, found);
            }
        }
        _ => {}
    }
}

fn load_and_dereference(
    spec_file: &Path,
    common_types_dir: Option<&Path>,
) -> Result<(Value, IndexMap<String, Value>)> {
    let src = std::fs::read_to_string(spec_file)
        .with_context(|| format!("failed to read {}", spec_file.display()))?;
    let parsed = parse_yaml_or_json(&src, spec_file)?;
    let spec_dir = spec_file.parent().unwrap_or(spec_file);
    let mut defs = IndexMap::new();
    let dereffed = dereference(
        &parsed.clone(),
        parsed,
        spec_dir,
        common_types_dir,
        &mut defs,
        0,
    );
    Ok((dereffed, defs))
}

// ---------------------------------------------------------------------------
// Scope normalization
// ---------------------------------------------------------------------------

fn normalize_scope(scope: &str) -> String {
    let s = scope.trim();
    if s.is_empty() {
        return s.to_owned();
    }

    // urn:godaddy:services:commerce.X:Y
    if let Some(rest) = s.strip_prefix("urn:godaddy:services:commerce.")
        && let Some(colon) = rest.find(':')
    {
        let domain = rest[..colon].to_lowercase();
        let action = normalize_scope_action(&rest[colon + 1..]);
        return format!("commerce.{domain}:{action}");
    }

    // https://uri.godaddy.com/services/commerce/X/Y
    if let Some(rest) = s.strip_prefix("https://uri.godaddy.com/services/commerce/")
        && let Some(slash) = rest.rfind('/')
    {
        let domain = rest[..slash].to_lowercase();
        let action = normalize_scope_action(&rest[slash + 1..]);
        return format!("commerce.{domain}:{action}");
    }

    // commerce.X:Y — already in target format, just normalize action
    let s_lower = s.to_lowercase();
    if let Some(rest) = s_lower.strip_prefix("commerce.")
        && let Some(colon) = rest.find(':')
    {
        let domain = rest[..colon].to_owned();
        let action = normalize_scope_action(&rest[colon + 1..]);
        return format!("commerce.{domain}:{action}");
    }

    s.to_owned()
}

fn normalize_scope_action(action: &str) -> String {
    let a = action.trim().to_lowercase();
    if a == "read-write" {
        "write".to_owned()
    } else {
        a
    }
}

fn extract_scopes(security: &Value) -> Vec<String> {
    let arr = match security.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };
    let mut scopes = Vec::new();
    for entry in arr {
        if let Some(map) = entry.as_object() {
            for scope_list in map.values() {
                if let Some(list) = scope_list.as_array() {
                    for s in list {
                        if let Some(raw) = s.as_str() {
                            let normalized = normalize_scope(raw);
                            if !normalized.is_empty() && !scopes.contains(&normalized) {
                                scopes.push(normalized);
                            }
                        }
                    }
                }
            }
        }
    }
    scopes
}

// ---------------------------------------------------------------------------
// OpenAPI processing
// ---------------------------------------------------------------------------

fn resolve_base_url(servers: &Value) -> String {
    let arr = match servers.as_array() {
        Some(a) if !a.is_empty() => a,
        _ => return String::new(),
    };
    let server = &arr[0];
    let mut url = server
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    if let Some(vars) = server.get("variables").and_then(|v| v.as_object()) {
        for (key, var) in vars {
            if let Some(default) = var.get("default").and_then(|v| v.as_str()) {
                url = url.replace(&format!("{{{key}}}"), default);
            }
        }
    }
    url
}

fn process_parameter(param: &Value) -> Option<CatalogParameter> {
    let name = param.get("name")?.as_str()?.to_owned();
    let location = param.get("in")?.as_str()?.to_owned();
    let required = param
        .get("required")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let description = param
        .get("description")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let schema = param.get("schema").cloned();
    Some(CatalogParameter {
        name,
        location,
        required,
        description,
        schema,
    })
}

fn process_request_body(rb: &Value) -> Option<CatalogRequestBody> {
    // Skip $ref objects that weren't resolved
    if rb.get("$ref").is_some() {
        return None;
    }
    let required = rb
        .get("required")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let description = rb
        .get("description")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let content = rb.get("content")?.as_object()?;
    let content_type = content
        .keys()
        .next()
        .cloned()
        .unwrap_or_else(|| "application/json".to_owned());
    let schema = content
        .get(&content_type)
        .and_then(|ct| ct.get("schema"))
        .cloned();
    Some(CatalogRequestBody {
        required,
        description,
        content_type,
        schema,
    })
}

fn process_responses(responses: &Value) -> HashMap<String, CatalogResponse> {
    let mut map = HashMap::new();
    let obj = match responses.as_object() {
        Some(o) => o,
        None => return map,
    };
    for (status, resp) in obj {
        if let Some(ref_val) = resp.get("$ref").and_then(|v| v.as_str()) {
            let schema = if ref_val.starts_with("#/$defs/") {
                Some(resp.clone())
            } else {
                None
            };
            map.insert(
                status.clone(),
                CatalogResponse {
                    description: format!("See {ref_val}"),
                    schema,
                },
            );
            continue;
        }
        let description = resp
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let schema = resp
            .get("content")
            .and_then(|c| c.as_object())
            .and_then(|c| c.values().next())
            .and_then(|ct| ct.get("schema"))
            .cloned();
        map.insert(
            status.clone(),
            CatalogResponse {
                description,
                schema,
            },
        );
    }
    map
}

fn operation_id_fallback(method: &str, path: &str) -> String {
    let slug: String = path
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    format!("{method}_{slug}")
}

#[allow(clippy::too_many_arguments)]
fn process_operation(
    _spec: &Value,
    spec_file: &Path,
    method: &str,
    path_str: &str,
    operation: &Value,
    path_params: &[Value],
    common_types_dir: Option<&Path>,
    graphql_cache: &mut HashMap<String, CatalogGraphqlSchema>,
) -> CatalogEndpoint {
    let operation_id = operation
        .get("operationId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let operation_id = if operation_id.is_empty() {
        operation_id_fallback(method, path_str)
    } else {
        operation_id
    };

    let summary = operation
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let description = operation
        .get("description")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    // Merge path-level and operation-level parameters
    let op_params = operation
        .get("parameters")
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    let all_params: Vec<&Value> = path_params.iter().chain(op_params.iter()).collect();
    let parameters: Vec<CatalogParameter> = all_params
        .iter()
        .filter_map(|p| {
            // Skip $ref params that weren't resolved
            if p.get("$ref").is_some() {
                return None;
            }
            process_parameter(p)
        })
        .collect();
    let parameters = if parameters.is_empty() {
        None
    } else {
        Some(parameters)
    };

    let request_body = operation.get("requestBody").and_then(process_request_body);
    let responses = operation
        .get("responses")
        .map(process_responses)
        .unwrap_or_default();

    let scopes = operation
        .get("security")
        .map(extract_scopes)
        .unwrap_or_default();

    // GraphQL schema extension
    let graphql = operation
        .get("x-godaddy-graphql-schema")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .and_then(|schema_ref| {
            let spec_dir = spec_file.parent().unwrap_or(spec_file);
            let resolved = spec_dir.join(schema_ref);
            let cache_key = resolved.to_string_lossy().into_owned();
            if let Some(cached) = graphql_cache.get(&cache_key) {
                // Return a clone with original schema_ref
                Some(CatalogGraphqlSchema {
                    schema_ref: schema_ref.to_owned(),
                    operation_count: cached.operation_count,
                    operations: cached
                        .operations
                        .iter()
                        .map(|op| CatalogGraphqlOperation {
                            name: op.name.clone(),
                            kind: op.kind.clone(),
                            return_type: op.return_type.clone(),
                            description: op.description.clone(),
                            deprecated: op.deprecated,
                            deprecation_reason: op.deprecation_reason.clone(),
                            args: op
                                .args
                                .iter()
                                .map(|a| CatalogGraphqlArgument {
                                    name: a.name.clone(),
                                    arg_type: a.arg_type.clone(),
                                    required: a.required,
                                    description: a.description.clone(),
                                    default_value: a.default_value.clone(),
                                })
                                .collect(),
                        })
                        .collect(),
                })
            } else {
                match load_graphql_schema(&resolved, schema_ref, common_types_dir) {
                    Ok(gql) => {
                        graphql_cache.insert(
                            cache_key,
                            CatalogGraphqlSchema {
                                schema_ref: gql.schema_ref.clone(),
                                operation_count: gql.operation_count,
                                operations: gql
                                    .operations
                                    .iter()
                                    .map(|op| CatalogGraphqlOperation {
                                        name: op.name.clone(),
                                        kind: op.kind.clone(),
                                        return_type: op.return_type.clone(),
                                        description: op.description.clone(),
                                        deprecated: op.deprecated,
                                        deprecation_reason: op.deprecation_reason.clone(),
                                        args: op
                                            .args
                                            .iter()
                                            .map(|a| CatalogGraphqlArgument {
                                                name: a.name.clone(),
                                                arg_type: a.arg_type.clone(),
                                                required: a.required,
                                                description: a.description.clone(),
                                                default_value: a.default_value.clone(),
                                            })
                                            .collect(),
                                    })
                                    .collect(),
                            },
                        );
                        Some(gql)
                    }
                    Err(e) => {
                        eprintln!("WARNING: failed to load GraphQL schema {schema_ref}: {e}");
                        None
                    }
                }
            }
        });

    CatalogEndpoint {
        operation_id,
        method: method.to_uppercase(),
        path: path_str.to_owned(),
        summary,
        description,
        parameters,
        request_body,
        responses,
        scopes,
        graphql,
    }
}

fn process_spec(
    spec: Value,
    domain: &str,
    spec_file: &Path,
    common_types_dir: Option<&Path>,
) -> CatalogDomain {
    let base_url = spec
        .get("servers")
        .map(resolve_base_url)
        .unwrap_or_default();

    let title = spec
        .get("info")
        .and_then(|i| i.get("title"))
        .and_then(|v| v.as_str())
        .unwrap_or(domain)
        .to_owned();

    let description = spec
        .get("info")
        .and_then(|i| i.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();

    let version = spec
        .get("info")
        .and_then(|i| i.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();

    let mut endpoints = Vec::new();
    let mut graphql_cache: HashMap<String, CatalogGraphqlSchema> = HashMap::new();

    if let Some(paths) = spec.get("paths").and_then(|v| v.as_object()) {
        for (path_str, path_item) in paths {
            let path_params: Vec<Value> = path_item
                .get("parameters")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            for method in HTTP_METHODS {
                if let Some(operation) = path_item.get(*method) {
                    endpoints.push(process_operation(
                        &spec,
                        spec_file,
                        method,
                        path_str,
                        operation,
                        &path_params,
                        common_types_dir,
                        &mut graphql_cache,
                    ));
                }
            }
        }
    }

    CatalogDomain {
        name: domain.to_owned(),
        title,
        description,
        version,
        base_url,
        endpoints,
    }
}

// ---------------------------------------------------------------------------
// GraphQL parsing
// ---------------------------------------------------------------------------

fn load_graphql_schema(
    path: &Path,
    schema_ref: &str,
    _common_types_dir: Option<&Path>,
) -> Result<CatalogGraphqlSchema> {
    let src = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read GraphQL schema {}", path.display()))?;

    let operations = parse_graphql_operations(&src).unwrap_or_else(|e| {
        eprintln!(
            "WARNING: failed to parse GraphQL schema {}: {e}",
            path.display()
        );
        Vec::new()
    });

    Ok(CatalogGraphqlSchema {
        schema_ref: schema_ref.to_owned(),
        operation_count: operations.len(),
        operations,
    })
}

fn parse_graphql_operations(source: &str) -> Result<Vec<CatalogGraphqlOperation>> {
    use graphql_parser::schema::{Definition, TypeDefinition};

    let doc = graphql_parser::parse_schema::<String>(source)
        .map_err(|e| anyhow::anyhow!("GraphQL parse error: {e}"))?;

    let mut operations: Vec<CatalogGraphqlOperation> = Vec::new();

    for def in &doc.definitions {
        let type_def = match def {
            Definition::TypeDefinition(td) => td,
            _ => continue,
        };
        let obj = match type_def {
            TypeDefinition::Object(o) => o,
            _ => continue,
        };
        let kind = match obj.name.as_str() {
            "Query" => "query",
            "Mutation" => "mutation",
            _ => continue,
        };

        for field in &obj.fields {
            let deprecated = field.directives.iter().any(|d| d.name == "deprecated");
            let deprecation_reason = field
                .directives
                .iter()
                .find(|d| d.name == "deprecated")
                .and_then(|d| d.arguments.iter().find(|(k, _)| k == "reason"))
                .and_then(|(_, v)| {
                    if let graphql_parser::query::Value::String(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                });

            let args: Vec<CatalogGraphqlArgument> = field
                .arguments
                .iter()
                .map(|arg| {
                    let type_str = graphql_type_to_string(&arg.value_type);
                    let required =
                        matches!(arg.value_type, graphql_parser::schema::Type::NonNullType(_))
                            && arg.default_value.is_none();
                    let default_value = arg.default_value.as_ref().map(|v| format!("{v}"));
                    CatalogGraphqlArgument {
                        name: arg.name.clone(),
                        arg_type: type_str,
                        required,
                        description: arg.description.clone(),
                        default_value,
                    }
                })
                .collect();

            operations.push(CatalogGraphqlOperation {
                name: field.name.clone(),
                kind: kind.to_owned(),
                return_type: graphql_type_to_string(&field.field_type),
                description: field.description.clone(),
                deprecated,
                deprecation_reason,
                args,
            });
        }
    }

    // Sort: queries before mutations, then alphabetical
    operations.sort_by(|a, b| {
        if a.kind == b.kind {
            a.name.cmp(&b.name)
        } else if a.kind == "query" {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    });

    Ok(operations)
}

fn graphql_type_to_string<'a, T: graphql_parser::query::Text<'a>>(
    t: &graphql_parser::schema::Type<'a, T>,
) -> String
where
    T::Value: std::fmt::Display,
{
    use graphql_parser::schema::Type;
    match t {
        Type::NamedType(name) => name.as_ref().to_string(),
        Type::NonNullType(inner) => format!("{}!", graphql_type_to_string(inner.as_ref())),
        Type::ListType(inner) => format!("[{}]", graphql_type_to_string(inner.as_ref())),
    }
}

// ---------------------------------------------------------------------------
// GraphQL-only domain synthesis
// ---------------------------------------------------------------------------

fn synthesize_graphql_domain(
    source: &SpecSource,
    common_types_dir: Option<&Path>,
) -> CatalogDomain {
    let gql = load_graphql_schema(&source.spec_file, "./schema.graphql", common_types_dir)
        .unwrap_or_else(|e| {
            eprintln!(
                "WARNING: failed to load GraphQL schema for {}: {e}",
                source.domain
            );
            CatalogGraphqlSchema {
                schema_ref: "./schema.graphql".to_owned(),
                operation_count: 0,
                operations: Vec::new(),
            }
        });

    let op_count = gql.operation_count;
    CatalogDomain {
        name: source.domain.clone(),
        title: format!("{} GraphQL API", source.domain),
        description: format!("GraphQL API with {op_count} operations"),
        version: source
            .spec_version
            .strip_prefix('v')
            .unwrap_or(&source.spec_version)
            .to_owned(),
        base_url: String::new(),
        endpoints: vec![CatalogEndpoint {
            operation_id: "graphql".to_owned(),
            method: "POST".to_owned(),
            path: "/graphql".to_owned(),
            summary: "GraphQL API".to_owned(),
            description: Some(format!("GraphQL endpoint with {op_count} operations")),
            parameters: None,
            request_body: None,
            responses: {
                let mut r = HashMap::new();
                r.insert(
                    "200".to_owned(),
                    CatalogResponse {
                        description: "GraphQL response".to_owned(),
                        schema: None,
                    },
                );
                r
            },
            scopes: Vec::new(),
            graphql: Some(gql),
        }],
    }
}

// ---------------------------------------------------------------------------
// Stale file removal
// ---------------------------------------------------------------------------

fn remove_stale_json(output_dir: &Path, active: &HashSet<String>) -> Result<()> {
    for entry in std::fs::read_dir(output_dir).context("failed to read output dir")? {
        let entry = entry.context("dir entry error")?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "manifest.json" || !name.ends_with(".json") {
            continue;
        }
        if !active.contains(&name) {
            std::fs::remove_file(entry.path())
                .with_context(|| format!("failed to remove stale {name}"))?;
            eprintln!("Removed stale: {name}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Output directory resolution
// ---------------------------------------------------------------------------

fn resolve_output_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR resolves to rust/tools/generate-api-catalog at build time
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("../../schemas/api")
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let output_dir = resolve_output_dir();
    std::fs::create_dir_all(&output_dir).context("failed to create output dir")?;

    let source_manifest = load_source_manifest()?;
    eprintln!("Discovering specification repositories...");
    let (mut sources, tmpdir) = discover_spec_sources(&source_manifest)?;
    sources.extend(local_spec_sources(&source_manifest)?);

    if sources.is_empty() {
        bail!("no specification repositories discovered — refusing to overwrite catalog output");
    }

    let ct_dir = tmpdir.join("__common-types");
    let common_types: Option<&Path> = if ct_dir.exists() { Some(&ct_dir) } else { None };

    let mut manifest = CatalogManifest {
        generated: chrono::Utc::now().to_rfc3339(),
        domains: HashMap::new(),
    };
    let mut active_files: HashSet<String> = HashSet::new();
    let mut total_endpoints = 0usize;

    for source in &sources {
        eprintln!(
            "Processing {} ({}/{})...",
            source.domain, source.repo_name, source.spec_version
        );

        let (catalog, defs) = if source.graphql_only {
            (
                synthesize_graphql_domain(source, common_types),
                IndexMap::new(),
            )
        } else {
            let (spec, defs) = load_and_dereference(&source.spec_file, common_types)
                .with_context(|| format!("failed to dereference {}", source.repo_name))?;
            let mut unresolved = collect_unresolved_refs(&spec);
            for def_val in defs.values() {
                unresolved.extend(collect_unresolved_refs(def_val));
            }
            unresolved.sort();
            unresolved.dedup();
            if !unresolved.is_empty() {
                bail!(
                    "{} unresolved $ref(s) remain in {} after dereferencing:\n  {}",
                    unresolved.len(),
                    source.repo_name,
                    unresolved.join("\n  ")
                );
            }
            (
                process_spec(spec, &source.domain, &source.spec_file, common_types),
                defs,
            )
        };

        let filename = format!("{}.json", source.domain);
        let out_path = output_dir.join(&filename);
        let mut catalog_value =
            serde_json::to_value(&catalog).context("failed to convert catalog to value")?;
        if !defs.is_empty() {
            let defs_map: serde_json::Map<String, Value> = defs.into_iter().collect();
            catalog_value["$defs"] = Value::Object(defs_map);
        }
        let json = serde_json::to_string_pretty(&catalog_value)
            .context("failed to serialize catalog domain")?;
        std::fs::write(&out_path, &json)
            .with_context(|| format!("failed to write {}", out_path.display()))?;

        let ep_count = catalog.endpoints.len();
        eprintln!(
            "  {} endpoints from {} v{}",
            ep_count, catalog.title, catalog.version
        );
        total_endpoints += ep_count;

        manifest.domains.insert(
            source.domain.clone(),
            ManifestEntry {
                file: filename.clone(),
                title: catalog.title,
                endpoint_count: ep_count,
            },
        );
        active_files.insert(filename);
    }

    // Remove stale *.json files
    remove_stale_json(&output_dir, &active_files)?;

    // Write manifest
    let manifest_path = output_dir.join("manifest.json");
    let manifest_json =
        serde_json::to_string_pretty(&manifest).context("failed to serialize manifest")?;
    std::fs::write(&manifest_path, &manifest_json)
        .with_context(|| format!("failed to write {}", manifest_path.display()))?;

    // Cleanup temp dir
    if let Err(e) = std::fs::remove_dir_all(&tmpdir) {
        eprintln!(
            "WARNING: failed to clean up temp dir {}: {e}",
            tmpdir.display()
        );
    }

    eprintln!(
        "\nGenerated API catalog: {} domains, {total_endpoints} endpoints",
        manifest.domains.len()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn dereference_resolves_relative_file_ref() {
        let dir = tempfile::tempdir().expect("tempdir");
        let model_dir = dir.path().join("schemas").join("models");
        fs::create_dir_all(&model_dir).expect("create model dir");

        fs::write(
            model_dir.join("Widget.yaml"),
            "type: object\nproperties:\n  id:\n    type: string\n",
        )
        .expect("write model");

        let spec_path = dir.path().join("schemas").join("openapi.yaml");
        fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Widget:\n      $ref: './models/Widget.yaml'\n",
        )
        .expect("write spec");

        let (result, _defs) = load_and_dereference(&spec_path, None).expect("dereference");
        let unresolved = collect_unresolved_refs(&result);
        assert!(
            unresolved.is_empty(),
            "expected zero unresolved $refs, got: {unresolved:?}"
        );
    }

    #[test]
    fn dereference_resolves_ref_in_sibling_of_schemas_dir() {
        // Models at {dir}/models/ rather than {dir}/schemas/models/ (sibling layout)
        let dir = tempfile::tempdir().expect("tempdir");
        let model_dir = dir.path().join("models");
        fs::create_dir_all(&model_dir).expect("create model dir");
        let schemas_dir = dir.path().join("schemas");
        fs::create_dir_all(&schemas_dir).expect("create schemas dir");

        fs::write(
            model_dir.join("Widget.yaml"),
            "type: object\nproperties:\n  id:\n    type: string\n",
        )
        .expect("write model");

        let spec_path = schemas_dir.join("openapi.yaml");
        fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Widget:\n      $ref: './models/Widget.yaml'\n",
        )
        .expect("write spec");

        let (result, _defs) = load_and_dereference(&spec_path, None).expect("dereference");
        let unresolved = collect_unresolved_refs(&result);
        assert!(
            unresolved.is_empty(),
            "expected zero unresolved $refs with sibling layout, got: {unresolved:?}"
        );
    }

    #[test]
    fn collect_unresolved_refs_finds_external_refs() {
        let value = serde_json::json!({
            "components": {
                "schemas": {
                    "Foo": { "$ref": "./models/Foo.yaml" },
                    "Bar": { "$ref": "#/components/schemas/Baz" },
                    "Qux": { "type": "string" }
                }
            }
        });
        let unresolved = collect_unresolved_refs(&value);
        assert_eq!(unresolved, vec!["./models/Foo.yaml"]);
    }

    #[test]
    fn dereference_lifts_external_ref_to_defs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let model_dir = dir.path().join("schemas").join("models");
        fs::create_dir_all(&model_dir).expect("create model dir");

        fs::write(
            model_dir.join("Widget.yaml"),
            "type: object\nproperties:\n  id:\n    type: string\n",
        )
        .expect("write model");

        let spec_path = dir.path().join("schemas").join("openapi.yaml");
        fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Widget:\n      $ref: './models/Widget.yaml'\n",
        )
        .expect("write spec");

        let (_result, defs) = load_and_dereference(&spec_path, None).expect("dereference");
        assert!(
            defs.contains_key("Widget"),
            "Widget should be lifted to $defs"
        );
        let widget = &defs["Widget"];
        assert_eq!(widget.get("type").and_then(|v| v.as_str()), Some("object"));
    }

    #[test]
    fn dereference_deduplicates_same_external_ref() {
        let dir = tempfile::tempdir().expect("tempdir");
        let model_dir = dir.path().join("schemas").join("models");
        fs::create_dir_all(&model_dir).expect("create model dir");

        fs::write(
            model_dir.join("Tag.yaml"),
            "type: object\nproperties:\n  name:\n    type: string\n",
        )
        .expect("write model");

        // Spec references Tag from two different endpoints
        let spec_path = dir.path().join("schemas").join("openapi.yaml");
        fs::write(
            &spec_path,
            r#"openapi: '3.0.0'
paths:
  /a:
    get:
      operationId: getA
      responses:
        '200':
          content:
            application/json:
              schema:
                $ref: './models/Tag.yaml'
  /b:
    get:
      operationId: getB
      responses:
        '200':
          content:
            application/json:
              schema:
                $ref: './models/Tag.yaml'
"#,
        )
        .expect("write spec");

        let (result, defs) = load_and_dereference(&spec_path, None).expect("dereference");
        assert!(defs.contains_key("Tag"), "Tag should be in $defs");

        // Both response schemas should be $ref pointers, not inlined
        let paths = result.get("paths").expect("paths");
        let schema_a =
            paths["/a"]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
                .as_object()
                .expect("schema_a object");
        assert_eq!(
            schema_a.get("$ref").and_then(|v| v.as_str()),
            Some("#/$defs/Tag"),
            "schema_a should be a local $defs ref"
        );
        let schema_b =
            paths["/b"]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
                .as_object()
                .expect("schema_b object");
        assert_eq!(
            schema_b.get("$ref").and_then(|v| v.as_str()),
            Some("#/$defs/Tag"),
            "schema_b should be a local $defs ref"
        );
    }

    #[test]
    fn dereference_lifts_component_schema_ref() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spec_path = dir.path().join("openapi.yaml");
        fs::write(
            &spec_path,
            r#"openapi: '3.0.0'
components:
  schemas:
    Item:
      type: object
      properties:
        id:
          type: string
paths:
  /items:
    get:
      operationId: listItems
      responses:
        '200':
          content:
            application/json:
              schema:
                type: array
                items:
                  $ref: '#/components/schemas/Item'
"#,
        )
        .expect("write spec");

        let (result, defs) = load_and_dereference(&spec_path, None).expect("dereference");

        // Item should be lifted to $defs
        assert!(defs.contains_key("Item"), "Item should be in $defs");

        // The response schema items should be a local $ref
        let schema = result["paths"]["/items"]["get"]["responses"]["200"]["content"]
            ["application/json"]["schema"]
            .as_object()
            .expect("schema object");
        let items_ref = schema["items"]
            .get("$ref")
            .and_then(|v| v.as_str())
            .expect("items.$ref");
        assert_eq!(items_ref, "#/$defs/Item");
    }

    #[test]
    fn dereference_circular_ref_guard() {
        // Schema A references schema B which references schema A — must not infinite-loop
        let dir = tempfile::tempdir().expect("tempdir");
        let spec_path = dir.path().join("openapi.yaml");
        fs::write(
            &spec_path,
            r#"openapi: '3.0.0'
components:
  schemas:
    NodeA:
      type: object
      properties:
        child:
          $ref: '#/components/schemas/NodeB'
    NodeB:
      type: object
      properties:
        parent:
          $ref: '#/components/schemas/NodeA'
"#,
        )
        .expect("write spec");

        // Must complete without stack overflow or panic
        let (_result, defs) = load_and_dereference(&spec_path, None).expect("dereference");
        assert!(defs.contains_key("NodeA") || defs.contains_key("NodeB"));
    }

    #[test]
    fn derive_defs_key_for_path_extracts_stem() {
        assert_eq!(derive_defs_key_for_path("./models/Order.yaml"), "Order");
        assert_eq!(
            derive_defs_key_for_path("../common-types/error.json"),
            "error"
        );
        assert_eq!(
            derive_defs_key_for_path("https://schemas.api.godaddy.com/v1/json/address.json"),
            "address"
        );
        assert_eq!(
            derive_defs_key_for_path("./Bulk_Ingestion.yaml"),
            "Bulk_Ingestion"
        );
        // Fragment with component name takes precedence over file stem
        assert_eq!(
            derive_defs_key_for_path("./common.yaml#/components/schemas/Foo"),
            "Foo"
        );
        assert_eq!(
            derive_defs_key_for_path("./shared.yaml#/definitions/Bar"),
            "Bar"
        );
    }

    #[test]
    fn source_manifest_defines_the_reconciled_catalog_domains() {
        let manifest = load_source_manifest().expect("load source manifest");
        let expected = manifest.expected_domains();

        assert_eq!(expected.len(), 21);
        assert_eq!(manifest.remote.len(), 20);
        assert_eq!(
            manifest
                .local
                .iter()
                .map(|source| source.domain.as_str())
                .collect::<Vec<_>>(),
            ["hosting-nodejs"]
        );
        assert_eq!(
            manifest.legacy_typescript.rust_only_domains,
            ["hosting-nodejs"]
        );
    }

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

    /// Drift guard: the committed catalog files and `manifest.json` must match the
    /// intentional source-manifest contract, and every domain's endpoint count must
    /// agree between its file and the manifest. Catches accidental drift (a domain
    /// added/removed, a hand-edited catalog, a stale manifest) on every `cargo test`
    /// run — no network or credentials required.
    #[test]
    fn committed_catalog_matches_expected_domains_and_manifest() {
        let dir = resolve_output_dir();
        let source_manifest = load_source_manifest().expect("load source manifest");

        // Domain files present on disk (excluding manifest.json).
        let mut files: Vec<String> = std::fs::read_dir(&dir)
            .expect("read schemas/api dir")
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                match n.strip_suffix(".json") {
                    Some(stem) if n != "manifest.json" => Some(stem.to_owned()),
                    _ => None,
                }
            })
            .collect();
        files.sort();

        let expected = source_manifest.expected_domains();

        assert_eq!(
            files, expected,
            "catalog domain files drifted from api-catalog-sources.json"
        );

        // manifest.json must list exactly the same domains...
        let manifest: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("manifest.json")).expect("read manifest.json"),
        )
        .expect("parse manifest.json");
        let domains = manifest["domains"]
            .as_object()
            .expect("manifest.domains is an object");

        let mut manifest_domains: Vec<String> = domains.keys().cloned().collect();
        manifest_domains.sort();
        assert_eq!(
            manifest_domains, expected,
            "manifest.json domains differ from api-catalog-sources.json / catalog files"
        );

        // ...and each domain's endpoint count must match between file and manifest.
        for domain in &expected {
            let catalog: Value = serde_json::from_str(
                &std::fs::read_to_string(dir.join(format!("{domain}.json")))
                    .unwrap_or_else(|e| panic!("read {domain}.json: {e}")),
            )
            .unwrap_or_else(|e| panic!("parse {domain}.json: {e}"));

            // Fail loudly on a structurally broken file rather than comparing two
            // silent zeros (which would hide a missing endpoints array / count).
            let actual = catalog["endpoints"]
                .as_array()
                .unwrap_or_else(|| panic!("'{domain}.json' has no endpoints array"))
                .len();
            let manifest_count = domains[domain]["endpointCount"]
                .as_u64()
                .unwrap_or_else(|| panic!("manifest.json missing endpointCount for '{domain}'"))
                as usize;
            assert_eq!(
                actual, manifest_count,
                "endpoint-count drift for '{domain}': file has {actual}, manifest says {manifest_count}"
            );
        }
    }
}
