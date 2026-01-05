import type { BundleRule } from "../../types.ts";

/**
 * SEC107: inspector module usage in bundled code (TWO-PASS DETECTION).
 *
 * Detects inspector module usage which can enable debugging/remote access.
 * Uses two-pass detection: only reports usage patterns if import/require signals are found.
 *
 * **Severity**: block
 * **Source Rule**: SEC007
 */
export const SEC107_INSPECTOR: BundleRule = {
	id: "SEC107",
	severity: "block",
	title: "inspector module usage in bundle",
	description:
		"Bundled code uses inspector module which can enable remote debugging access",
	sourceRuleId: "SEC007",
	signalPatterns: [
		/require\s*\(\s*['"](?:node:)?inspector['"]\s*\)/g,
		/from\s*['"](?:node:)?inspector['"]/g,
		/import\s*\(\s*['"](?:node:)?inspector['"]\s*\)/g,
		/require_inspector\s*\(/g,
	],
	patterns: [
		/inspector\.open\s*\(/g,
		/inspector\.url\s*\(/g,
		/inspector\.waitForDebugger\s*\(/g,
		/inspector\[['"]open['"]]\s*\(/g,
	],
};
