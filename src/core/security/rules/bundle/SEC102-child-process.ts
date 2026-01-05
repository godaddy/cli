import type { BundleRule } from "../../types.ts";

/**
 * SEC102: child_process usage in bundled code (TWO-PASS DETECTION).
 *
 * Detects child_process module usage including bundler helpers and obfuscated variants.
 * Uses two-pass detection: only reports usage patterns if import/require signals are found.
 *
 * **Severity**: block
 * **Source Rule**: SEC002
 */
export const SEC102_CHILD_PROCESS: BundleRule = {
	id: "SEC102",
	severity: "block",
	title: "child_process usage in bundle",
	description:
		"Bundled code imports and uses child_process module which can execute shell commands",
	sourceRuleId: "SEC002",
	signalPatterns: [
		// CommonJS require
		/require\s*\(\s*['"](?:node:)?child_process['"]\s*\)/g,

		// ESM import (use \s* not \s+ to match minified bundles with no space)
		/from\s*['"](?:node:)?child_process['"]/g,

		// Dynamic import
		/import\s*\(\s*['"](?:node:)?child_process['"]\s*\)/g,

		// Bundler helpers (webpack/esbuild)
		/require_child_process\s*\(/g,
		/__require\s*\(\s*['"]child_process['"]\s*\)/g,

		// esbuild dynamic require shim: var r=...; r("child_process")
		// Matches any single-letter function call with "child_process" string
		/\b[a-z]\s*\(\s*['"](?:node:)?child_process['"]\s*\)/g,

		// Obfuscated require (string concatenation) - fixed: use \s*\+\s* not [\s+]+
		/require\s*\(\s*['"](?:node:)?child['"]\s*\+\s*['"]_?process['"]\s*\)/g,

		// Decode→require obfuscation (narrow: only when decode feeds directly into require/import)
		/(?:require|import)\s*\(\s*(?:atob\([^)]*\)|Buffer\.from\([^,]+,\s*['"`]base64['"`]\)\.toString\(\))/g,
	],
	patterns: [
		// Common child_process methods
		/\bexec\s*\(/g,
		/\bexecSync\s*\(/g,
		/\bexecFile\s*\(/g,
		/\bexecFileSync\s*\(/g,
		/\bspawn\s*\(/g,
		/\bspawnSync\s*\(/g,
		/\bfork\s*\(/g,
	],
};
