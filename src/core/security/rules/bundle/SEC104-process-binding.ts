import type { BundleRule } from "../../types.ts";

/**
 * SEC104: process.binding/dlopen usage in bundled code.
 *
 * Detects access to Node.js internal bindings which can bypass security mechanisms.
 * No signal patterns needed - these APIs are always suspicious.
 *
 * **Severity**: block
 * **Source Rule**: SEC004
 */
export const SEC104_PROCESS_BINDING: BundleRule = {
	id: "SEC104",
	severity: "block",
	title: "process.binding/dlopen in bundle",
	description:
		"Bundled code accesses Node.js internal bindings which can bypass security",
	sourceRuleId: "SEC004",
	patterns: [
		/process\.binding\s*\(/g,
		/process\._linkedBinding\s*\(/g,
		/process\.dlopen\s*\(/g,
		/internalBinding\s*\(/g,
		// Bracket notation variants
		/process\[['"]binding['"]]\s*\(/g,
		/process\[['"]_linkedBinding['"]]\s*\(/g,
		/process\[['"]dlopen['"]]\s*\(/g,
	],
};
