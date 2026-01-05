import type { BundleRule } from "../../types.ts";

/**
 * SEC105: Native addon usage in bundled code (TWO-PASS DETECTION).
 *
 * Detects native addon loading via bindings/ffi-napi modules.
 * Uses two-pass detection: only reports usage patterns if import/require signals are found.
 *
 * **Severity**: block
 * **Source Rule**: SEC005
 */
export const SEC105_NATIVE_ADDON: BundleRule = {
	id: "SEC105",
	severity: "block",
	title: "Native addon usage in bundle",
	description:
		"Bundled code loads native addons which can bypass Node.js security",
	sourceRuleId: "SEC005",
	signalPatterns: [
		/require\s*\(\s*['"]bindings['"]\s*\)/g,
		/require\s*\(\s*['"]node-gyp['"]\s*\)/g,
		/require\s*\(\s*['"]ffi-napi['"]\s*\)/g,
		/require\s*\(\s*['"]node-addon-api['"]\s*\)/g,
		/from\s*['"]bindings['"]/g,
		/from\s*['"]ffi-napi['"]/g,
		/require_bindings\s*\(/g,
	],
	patterns: [/['"]\.node['"]/g, /\.node['"]?\s*\)/g, /process\.dlopen\s*\(/g],
};
