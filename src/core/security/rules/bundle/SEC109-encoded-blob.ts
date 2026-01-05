import type { BundleRule } from "../../types.ts";

/**
 * SEC109: Large encoded blobs in bundled code.
 *
 * Detects suspicious large base64/hex encoded data that could hide malicious payloads.
 * Focuses on decode patterns to reduce false positives.
 *
 * **Severity**: warn
 * **Source Rule**: SEC009
 */
export const SEC109_ENCODED_BLOB: BundleRule = {
	id: "SEC109",
	severity: "warn",
	title: "Large encoded blob in bundle",
	description:
		"Bundled code contains large base64/hex encoded data that could hide malicious payloads",
	sourceRuleId: "SEC009",
	patterns: [
		// Buffer.from base64 decoding (high confidence)
		/Buffer\.from\s*\(\s*['"][A-Za-z0-9+/]{200,}={0,2}['"]\s*,\s*['"]base64['"]\s*\)/g,

		// atob() decoding
		/atob\s*\(\s*['"][A-Za-z0-9+/]{200,}={0,2}['"]\s*\)/g,

		// Hex decoding
		/Buffer\.from\s*\(\s*['"][A-Fa-f0-9]{400,}['"]\s*,\s*['"]hex['"]\s*\)/g,
	],
};
