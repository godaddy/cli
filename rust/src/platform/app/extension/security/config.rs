//! Immutable strict security configuration for extension source scans.

use globset::{Glob, GlobSet, GlobSetBuilder};

use super::types::SecurityConfig;

pub fn security_config() -> SecurityConfig {
    SecurityConfig {
        trusted_domains: vec!["*.godaddy.com", "godaddy.com", "localhost", "127.0.0.1"],
        exclude: vec![
            "**/node_modules/**",
            "**/dist/**",
            "**/build/**",
            "**/__tests__/**",
        ],
    }
}

pub fn is_trusted_domain(url_or_domain: &str, config: &SecurityConfig) -> bool {
    let domain = extract_domain(url_or_domain);
    let normalized = domain.to_ascii_lowercase();
    for pattern in &config.trusted_domains {
        let pattern = pattern.to_ascii_lowercase();
        if let Some(base) = pattern.strip_prefix("*.") {
            if normalized == base
                || normalized
                    .strip_suffix(base)
                    .is_some_and(|prefix| prefix.ends_with('.'))
            {
                return true;
            }
        } else if normalized == pattern {
            return true;
        }
    }
    false
}

fn extract_domain(url_or_domain: &str) -> String {
    if let Ok(url) = url::Url::parse(url_or_domain) {
        let host = url.host_str().unwrap_or("");
        if !host.is_empty() {
            return host.to_owned();
        }
    }
    url_or_domain
        .split(':')
        .next()
        .unwrap_or(url_or_domain)
        .to_owned()
}

pub fn exclude_matcher(config: &SecurityConfig) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in &config.exclude {
        let glob = Glob::new(pattern).expect("valid security exclude glob");
        builder.add(glob);
    }
    builder.build().expect("valid security exclude globset")
}

pub fn should_exclude(path: &str, excludes: &GlobSet) -> bool {
    let normalized = path.replace('\\', "/");
    let trimmed = normalized.strip_prefix("./").unwrap_or(&normalized);
    excludes.is_match(trimmed)
}
