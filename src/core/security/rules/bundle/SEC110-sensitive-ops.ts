import type { BundleRule } from "../../types.ts";

/**
 * SEC110: Sensitive operations in bundled code (TWO-PASS DETECTION).
 *
 * Detects access to sensitive file paths, environment variables, and network operations.
 * Uses two-pass detection: only reports usage patterns if import/require signals are found.
 *
 * **Severity**: warn
 * **Source Rule**: SEC010
 */
export const SEC110_SENSITIVE_OPS: BundleRule = {
	id: "SEC110",
	severity: "warn",
	title: "Sensitive operations in bundle",
	description:
		"Bundled code accesses sensitive paths, environment variables, or network APIs",
	sourceRuleId: "SEC010",
	signalPatterns: [
		/require\s*\(\s*['"](?:node:)?net['"]\s*\)/g,
		/from\s*['"](?:node:)?net['"]/g,
		/require\s*\(\s*['"](?:node:)?fs['"]\s*\)/g,
		/from\s*['"](?:node:)?fs['"]/g,
	],
	patterns: [
		// Network connections
		/net\.connect\s*\(/g,
		/net\.createConnection\s*\(/g,

		// Environment variable access
		/process\.env\[/g,
		/process\.env\./g,

		// Sensitive file paths
		/['"]\/(etc\/passwd|etc\/shadow|\.ssh\/|\.aws\/)/g,
		/['"]~\/\.ssh\//g,
		/fs\.readFile(?:Sync)?\s*\(\s*['"][^'"]*\.env['"]/g,
	],
};
