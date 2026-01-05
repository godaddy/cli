/**
 * Tests for extension naming utilities.
 * Verifies hash computation, timestamp formatting, and name sanitization.
 */

import { createHash } from "node:crypto";
import {
	buildArtifactName,
	computeHash,
	formatTimestamp,
	sanitizeExtensionName,
	shortHash,
} from "@core/extension/naming";
import { describe, expect, it } from "vitest";

describe("naming utilities", () => {
	describe("computeHash", () => {
		it("should compute consistent SHA256 hash", () => {
			const content = Buffer.from("test content");
			const hash1 = computeHash(content);
			const hash2 = computeHash(content);

			expect(hash1).toBe(hash2);
			expect(hash1).toHaveLength(64);
		});

		it("should produce different hashes for different content", () => {
			const content1 = Buffer.from("content 1");
			const content2 = Buffer.from("content 2");

			const hash1 = computeHash(content1);
			const hash2 = computeHash(content2);

			expect(hash1).not.toBe(hash2);
		});

		it("should match expected SHA256 hash", () => {
			const content = Buffer.from("hello world");
			const expected = createHash("sha256").update(content).digest("hex");

			expect(computeHash(content)).toBe(expected);
		});
	});

	describe("shortHash", () => {
		it("should return first 6 characters", () => {
			const fullHash =
				"a3b2c1d4e5f6123456789abcdef0123456789abcdef0123456789abcdef012";
			expect(shortHash(fullHash)).toBe("a3b2c1");
			expect(shortHash(fullHash)).toHaveLength(6);
		});

		it("should work with any hash length", () => {
			expect(shortHash("abcdef123")).toBe("abcdef");
			expect(shortHash("abc")).toBe("abc");
		});
	});

	describe("formatTimestamp", () => {
		it("should format date as yyyymmddHHMMss", () => {
			const date = new Date("2025-01-28T14:30:22Z");
			expect(formatTimestamp(date)).toBe("20250128143022");
		});

		it("should pad single-digit values with zeros", () => {
			const date = new Date("2025-03-05T09:08:07Z");
			expect(formatTimestamp(date)).toBe("20250305090807");
		});

		it("should handle leap year", () => {
			const date = new Date("2024-02-29T23:59:59Z");
			expect(formatTimestamp(date)).toBe("20240229235959");
		});

		it("should handle month boundaries", () => {
			const date = new Date("2025-12-31T23:59:59Z");
			expect(formatTimestamp(date)).toBe("20251231235959");
		});

		it("should use current time when no date provided", () => {
			const timestamp = formatTimestamp();
			expect(timestamp).toMatch(/^\d{14}$/);
		});
	});

	describe("sanitizeExtensionName", () => {
		it("should replace @ and / with hyphens", () => {
			expect(sanitizeExtensionName("@scoped/extension")).toBe(
				"scoped-extension",
			);
		});

		it("should replace whitespace with hyphens", () => {
			expect(sanitizeExtensionName("my extension name")).toBe(
				"my-extension-name",
			);
		});

		it("should replace unsafe characters", () => {
			expect(sanitizeExtensionName('bad:name*with?chars"<>|')).toBe(
				"bad-name-with-chars",
			);
		});

		it("should lowercase the name", () => {
			expect(sanitizeExtensionName("MyExtension")).toBe("myextension");
		});

		it("should trim whitespace", () => {
			expect(sanitizeExtensionName("  extension  ")).toBe("extension");
		});

		it("should truncate to 100 characters", () => {
			const longName = "a".repeat(150);
			const sanitized = sanitizeExtensionName(longName);
			expect(sanitized).toHaveLength(100);
			expect(sanitized).toBe("a".repeat(100));
		});

		it("should handle backslashes", () => {
			expect(sanitizeExtensionName("path\\to\\extension")).toBe(
				"path-to-extension",
			);
		});

		it("should be cross-platform safe", () => {
			const unsafeName = '@my-org/my:extension*v2?test"file<tag>pipe|space ';
			const sanitized = sanitizeExtensionName(unsafeName);

			// Should not contain any unsafe characters
			expect(sanitized).not.toMatch(/[/\\:*?"<>|\s]/);
			expect(sanitized).toBe("my-org-my-extension-v2-test-file-tag-pipe-space");
		});

		it("should fallback to 'extension' when all characters are special", () => {
			expect(sanitizeExtensionName("@@@")).toBe("extension");
			expect(sanitizeExtensionName("///***???")).toBe("extension");
			expect(sanitizeExtensionName("   ")).toBe("extension");
		});
	});

	describe("buildArtifactName", () => {
		it("should build correct artifact name", () => {
			const name = buildArtifactName(
				"simple-extension",
				"1.0.0",
				"20250128143022",
				"a3b2c1",
			);
			expect(name).toBe("simple-extension-1.0.0-20250128143022-a3b2c1.mjs");
		});

		it("should sanitize extension name", () => {
			const name = buildArtifactName(
				"@scoped/extension",
				"0.5.2",
				"20250128143022",
				"f4e5d6",
			);
			expect(name).toBe("scoped-extension-0.5.2-20250128143022-f4e5d6.mjs");
		});

		it("should use default version when not provided", () => {
			const name = buildArtifactName(
				"extension",
				undefined,
				"20250128143022",
				"a3b2c1",
			);
			expect(name).toBe("extension-0.0.0-20250128143022-a3b2c1.mjs");
		});

		it("should handle complex names", () => {
			const name = buildArtifactName(
				"My Extension!",
				"2.1.0",
				"20250128143022",
				"abc123",
			);
			expect(name).toBe("my-extension-2.1.0-20250128143022-abc123.mjs");
		});

		it("should always end with .mjs", () => {
			const name = buildArtifactName(
				"test",
				"1.0.0",
				"20250128143022",
				"123456",
			);
			expect(name).toMatch(/\.mjs$/);
		});
	});
});
