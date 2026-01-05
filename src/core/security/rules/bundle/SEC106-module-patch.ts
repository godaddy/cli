import type { BundleRule } from "../../types.ts";

/**
 * SEC106: Module monkey-patching in bundled code (TWO-PASS DETECTION).
 *
 * Detects modification of module system internals.
 * Uses two-pass detection: only reports usage patterns if import/require signals are found.
 *
 * **Severity**: block
 * **Source Rule**: SEC006
 */
export const SEC106_MODULE_PATCH: BundleRule = {
	id: "SEC106",
	severity: "block",
	title: "Module monkey-patching in bundle",
	description:
		"Bundled code modifies module system internals which can hijack dependencies",
	sourceRuleId: "SEC006",
	signalPatterns: [
		/require\s*\(\s*['"]module['"]\s*\)/g,
		/from\s*['"]module['"]/g,
		/require_module\s*\(/g,
	],
	patterns: [
		/Module\._load\s*=/g,
		/Module\._resolveFilename\s*=/g,
		/Module\._extensions\[/g,
		/require\.cache\s*\[/g,
		/delete\s+require\.cache/g,
		// Bracket notation
		/Module\[['"]_load['"]]\s*=/g,
		/Module\[['"]_resolveFilename['"]]\s*=/g,
	],
};
