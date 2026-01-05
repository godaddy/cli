import picomatch from "picomatch";
import type { SecurityConfig } from "./types.ts";

const SECURITY_CONFIG: SecurityConfig = {
	mode: "strict",
	trustedDomains: ["*.godaddy.com", "godaddy.com", "localhost", "127.0.0.1"],
	exclude: [
		"**/node_modules/**",
		"**/dist/**",
		"**/build/**",
		"**/__tests__/**",
	],
};

export function getSecurityConfig(): SecurityConfig {
	return SECURITY_CONFIG;
}

export function isTrustedDomain(
	urlOrDomain: string,
	config: SecurityConfig,
): boolean {
	// Extract domain from full URL if needed
	let domain = urlOrDomain;
	try {
		const url = new URL(urlOrDomain);
		// URL constructor may succeed but return empty hostname for patterns like "localhost:3000"
		domain = url.hostname || urlOrDomain.split(":")[0];
	} catch {
		// Not a full URL, treat as domain and strip port if present
		domain = urlOrDomain.split(":")[0];
	}

	const normalizedDomain = domain.toLowerCase();

	for (const pattern of config.trustedDomains) {
		const normalizedPattern = pattern.toLowerCase();

		if (normalizedPattern.startsWith("*.")) {
			const baseDomain = normalizedPattern.slice(2);
			if (
				normalizedDomain === baseDomain ||
				normalizedDomain.endsWith(`.${baseDomain}`)
			) {
				return true;
			}
		} else if (normalizedDomain === normalizedPattern) {
			return true;
		}
	}

	return false;
}

export function shouldExcludeFile(
	filePath: string,
	config: SecurityConfig,
): boolean {
	const normalizedPath = filePath.replace(/\\/g, "/");

	for (const pattern of config.exclude) {
		if (matchesGlobPattern(normalizedPath, pattern)) {
			return true;
		}
	}

	return false;
}

/**
 * Glob pattern matcher using picomatch for robust pattern matching.
 * Supports **, *, ?, [], and other standard glob patterns.
 */
function matchesGlobPattern(path: string, pattern: string): boolean {
	const normalizedPath = path.startsWith("./") ? path.slice(2) : path;
	const matcher = picomatch(pattern);
	return matcher(normalizedPath);
}
