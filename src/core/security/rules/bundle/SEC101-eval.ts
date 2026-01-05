import type { BundleRule } from "../../types.ts";

/**
 * SEC101: Dynamic code execution (eval, Function constructor) in bundled code.
 *
 * Detects eval() and new Function() patterns including obfuscated variants
 * that may be present in bundled dependencies.
 *
 * **Severity**: block
 * **Source Rule**: SEC001
 */
export const SEC101_EVAL: BundleRule = {
	id: "SEC101",
	severity: "block",
	title: "Dynamic code execution in bundle",
	description:
		"Bundled code contains eval() or Function constructor which can execute arbitrary code",
	sourceRuleId: "SEC001",
	patterns: [
		// Direct calls with word-boundary protection (prevents matching "evaluation")
		/(?:^|[^\w$])eval\s*\(/g,
		/(?:^|[^\w$])new\s+Function\s*\(/g,

		// Global receiver variants
		/(?:globalThis|window|self)\.eval\s*\(/g,
		/(?:globalThis|window|self)\[['"]eval['"]]\s*\(/g,

		// Bracket notation obfuscation
		/\[['"]eval['"]]\s*\(/g,
		/\[['"]Function['"]]\s*\(/g,

		// Obfuscated decode→sink patterns (narrow scope to reduce FPs)
		// Only flag when decode is IMMEDIATELY passed to dangerous sink
		/(?:eval|Function|setTimeout|setInterval)\s*\(\s*(?:atob\([^)]*\)|Buffer\.from\([^,]+,\s*['"`]base64['"`]\))/g,

		// Hex-escaped string literals in eval (rare in legitimate code)
		/eval\s*\(\s*['"`](?:\\x[0-9A-Fa-f]{2}){8,}['"`]\s*\)/g,
	],
};
