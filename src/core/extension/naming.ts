/**
 * Pure artifact naming and hashing functions for extension bundling.
 * Provides utilities for generating consistent, safe artifact names.
 */

import { createHash } from "node:crypto";

/**
 * Computes SHA256 hash of buffer content.
 *
 * @param content - Buffer to hash
 * @returns Full SHA256 hash as hex string (64 characters)
 */
export function computeHash(content: Buffer): string {
	return createHash("sha256").update(content).digest("hex");
}

/**
 * Extracts first 6 characters of a hash for short identifiers.
 *
 * @param fullHash - Full hash string
 * @returns First 6 characters
 */
export function shortHash(fullHash: string): string {
	return fullHash.slice(0, 6);
}

/**
 * Formats a date as UTC timestamp in yyyymmddHHMMss format.
 *
 * @param date - Date to format (defaults to current time)
 * @returns Formatted timestamp string
 *
 * @example
 * ```ts
 * formatTimestamp(new Date("2025-01-28T14:30:22Z")) // "20250128143022"
 * ```
 */
export function formatTimestamp(date: Date = new Date()): string {
	const year = date.getUTCFullYear();
	const month = String(date.getUTCMonth() + 1).padStart(2, "0");
	const day = String(date.getUTCDate()).padStart(2, "0");
	const hours = String(date.getUTCHours()).padStart(2, "0");
	const minutes = String(date.getUTCMinutes()).padStart(2, "0");
	const seconds = String(date.getUTCSeconds()).padStart(2, "0");

	return `${year}${month}${day}${hours}${minutes}${seconds}`;
}

/**
 * Sanitizes extension name for use in filenames.
 * Replaces unsafe characters with hyphens, lowercases, and truncates.
 *
 * Unsafe characters: / \ : * ? " < > | and whitespace
 *
 * @param name - Extension name from package.json
 * @returns Sanitized name safe for cross-platform filenames
 *
 * @example
 * ```ts
 * sanitizeExtensionName("@scoped/extension") // "scoped-extension"
 * sanitizeExtensionName("My Extension!") // "my-extension"
 * sanitizeExtensionName("@@@") // "extension" (fallback for all special chars)
 * ```
 */
export function sanitizeExtensionName(name: string): string {
	// Replace unsafe chars with hyphens (includes @, !, and other special chars)
	const unsafeCharsRegex = /[/\\:*?"<>|\s@!]/g;
	const sanitized = name.replace(unsafeCharsRegex, "-");

	// Lowercase
	const lowercased = sanitized.toLowerCase();

	// Remove leading/trailing hyphens and whitespace
	const cleaned = lowercased.replace(/^[-\s]+|[-\s]+$/g, "");

	// Truncate to 100 chars for path safety
	const result = cleaned.slice(0, 100);

	// Guard against empty names (all special chars)
	return result || "extension";
}

/**
 * Builds complete artifact filename with sanitization.
 *
 * Format: {sanitizedName}-{version}-{timestamp}-{hash}.mjs
 *
 * @param name - Extension name from package.json
 * @param version - Extension version (defaults to "0.0.0")
 * @param timestamp - Timestamp string in yyyymmddHHMMss format
 * @param hash - Short hash (6 characters)
 * @returns Complete artifact filename
 *
 * @example
 * ```ts
 * buildArtifactName("@scoped/extension", "1.0.0", "20250128143022", "a3b2c1")
 * // Returns: "-scoped-extension-1.0.0-20250128143022-a3b2c1.mjs"
 * ```
 */
export function buildArtifactName(
	name: string,
	version: string | undefined,
	timestamp: string,
	hash: string,
): string {
	const sanitized = sanitizeExtensionName(name);
	const versionStr = version || "0.0.0";
	return `${sanitized}-${versionStr}-${timestamp}-${hash}.mjs`;
}
