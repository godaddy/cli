//! Shared types for pre-bundle source security scanning.

use std::collections::{HashMap, HashSet};

use crate::extension::{Finding, Severity};

#[derive(Debug, Default, Clone)]
pub struct AliasMaps {
    /// module → set of local names (default import / `const x = require(...)`)
    pub module_aliases: HashMap<String, HashSet<String>>,
    /// module → namespace alias (`import * as VM from 'vm'`)
    pub namespace_aliases: HashMap<String, String>,
    /// module → (imported name → local name)
    pub named_imports: HashMap<String, HashMap<String, String>>,
}

impl AliasMaps {
    pub fn is_alias_of(&self, local: &str, module: &str) -> bool {
        if self
            .module_aliases
            .get(module)
            .is_some_and(|set| set.contains(local))
        {
            return true;
        }
        if self
            .namespace_aliases
            .get(module)
            .is_some_and(|ns| ns == local)
        {
            return true;
        }
        if let Some(named) = self.named_imports.get(module) {
            return named.values().any(|v| v == local);
        }
        false
    }
}

#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub trusted_domains: Vec<&'static str>,
    pub exclude: Vec<&'static str>,
}

#[derive(Debug, Default)]
pub struct ScanSummary {
    pub total: usize,
    pub block: usize,
    pub warn: usize,
}

#[derive(Debug)]
pub struct ScanReport {
    pub findings: Vec<Finding>,
    pub blocked: bool,
    pub summary: ScanSummary,
    pub scanned_files: usize,
}

pub(crate) fn build_summary(findings: &[Finding]) -> ScanSummary {
    let mut summary = ScanSummary {
        total: findings.len(),
        ..ScanSummary::default()
    };
    for f in findings {
        match f.severity {
            Severity::Block => summary.block += 1,
            Severity::Warn => summary.warn += 1,
        }
    }
    summary
}
