import type { BundleRule } from "../../types.ts";

/**
 * SEC108: External URLs in bundled code (SPECIAL HANDLING).
 *
 * Detects HTTP(S) URLs to external domains.
 * Scanner must extract URLs and check against SecurityConfig.trustedDomains.
 *
 * **Severity**: warn
 * **Source Rule**: SEC008
 *
 * **Known Limitations**:
 * - Only detects statically defined URLs
 * - Template strings with variables are not caught
 * - String concatenation is not detected
 * - Obfuscated URLs (atob) are not detected
 * - Computed URLs are not detected
 */
export const SEC108_EXTERNAL_URL: BundleRule = {
	id: "SEC108",
	severity: "warn",
	title: "External URL in bundle",
	description: "Bundled code contains HTTP(S) URLs to external domains",
	sourceRuleId: "SEC008",
	signalPatterns: [
		/require\s*\(\s*['"](?:node:)?https?['"]\s*\)/g,
		/from\s*['"](?:node:)?https?['"]/g,
		/require\s*\(\s*['"]axios['"]\s*\)/g,
		/from\s*['"]axios['"]/g,
	],
	patterns: [
		// Extract all HTTP(S) URLs
		/https?:\/\/[^\s"'`<>]+/g,

		// new URL() constructor
		/new\s+URL\s*\(\s*['"]https?:[^'"]+['"]\s*\)/g,

		// fetch() calls
		/fetch\s*\(\s*['"]https?:[^'"]+['"]\s*\)/g,
	],
};
