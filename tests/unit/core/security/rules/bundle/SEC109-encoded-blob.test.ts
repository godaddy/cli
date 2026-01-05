import { SEC109_ENCODED_BLOB } from "@/core/security/rules/bundle/SEC109-encoded-blob.ts";
import { describe, expect, it } from "vitest";

describe("SEC109: large encoded blob detection (bundled)", () => {
	it("has correct metadata", () => {
		expect(SEC109_ENCODED_BLOB.id).toBe("SEC109");
		expect(SEC109_ENCODED_BLOB.severity).toBe("warn");
		expect(SEC109_ENCODED_BLOB.sourceRuleId).toBe("SEC009");
		expect(SEC109_ENCODED_BLOB.patterns.length).toBeGreaterThan(0);
	});

	it("detects large Buffer.from base64", () => {
		const largeBase64 = "A".repeat(200);
		const code = `Buffer.from("${largeBase64}", "base64")`;
		const hasMatch = SEC109_ENCODED_BLOB.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects large atob() call", () => {
		const largeBase64 = "B".repeat(200);
		const code = `atob("${largeBase64}")`;
		const hasMatch = SEC109_ENCODED_BLOB.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects large hex Buffer.from", () => {
		const largeHex = "A1B2C3D4".repeat(50); // 400 chars
		const code = `Buffer.from("${largeHex}", "hex")`;
		const hasMatch = SEC109_ENCODED_BLOB.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("does not match small base64 strings", () => {
		const code = 'Buffer.from("aGVsbG8=", "base64")'; // "hello"
		const hasMatch = SEC109_ENCODED_BLOB.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(false);
	});

	it("does not match raw strings without decode", () => {
		const largeString = "x".repeat(500);
		const code = `const data = "${largeString}";`;
		const hasMatch = SEC109_ENCODED_BLOB.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(false);
	});

	it("handles base64 with padding", () => {
		const largeBase64 = `${"C".repeat(200)}==`;
		const code = `Buffer.from("${largeBase64}", "base64")`;
		const hasMatch = SEC109_ENCODED_BLOB.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});
});
