import type { BundleRule } from "../../types.ts";

/**
 * SEC103: vm module usage in bundled code (TWO-PASS DETECTION).
 *
 * Detects vm module usage for dynamic code execution.
 * Uses two-pass detection: only reports usage patterns if import/require signals are found.
 *
 * **Severity**: block
 * **Source Rule**: SEC003
 */
export const SEC103_VM: BundleRule = {
	id: "SEC103",
	severity: "block",
	title: "vm module usage in bundle",
	description:
		"Bundled code imports and uses vm module which enables arbitrary code execution",
	sourceRuleId: "SEC003",
	signalPatterns: [
		/require\s*\(\s*['"](?:node:)?vm['"]\s*\)/g,
		/from\s*['"](?:node:)?vm['"]/g,
		/import\s*\(\s*['"](?:node:)?vm['"]\s*\)/g,
		/require_vm\s*\(/g,
		/__require\s*\(\s*['"]vm['"]\s*\)/g,
	],
	patterns: [
		/\bScript\s*\(/g,
		/\.runInNewContext\s*\(/g,
		/\.runInContext\s*\(/g,
		/\.runInThisContext\s*\(/g,
		/\.createContext\s*\(/g,
	],
};
