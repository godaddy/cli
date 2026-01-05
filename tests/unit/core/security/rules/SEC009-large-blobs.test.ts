import { buildAliasMaps } from "@/core/security/alias-builder.ts";
import { scanFile } from "@/core/security/engine.ts";
import { SEC009 } from "@/core/security/rules/SEC009-large-blobs.ts";
import type { SecurityConfig } from "@/core/security/types.ts";
import ts from "typescript";
import { describe, expect, it } from "vitest";

function createSourceFile(code: string): ts.SourceFile {
	return ts.createSourceFile(
		"test.ts",
		code,
		ts.ScriptTarget.Latest,
		true,
		ts.ScriptKind.TS,
	);
}

const mockConfig: SecurityConfig = {
	mode: "strict",
	trustedDomains: ["*.godaddy.com"],
	exclude: [],
};

describe("SEC009: Large encoded blob detection", () => {
	describe("large base64 strings", () => {
		it("should warn on Buffer.from with large base64 string", () => {
			const largeBase64 = "A".repeat(201);
			const code = `const data = Buffer.from("${largeBase64}", "base64");`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC009],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
			expect(findings[0].ruleId).toBe("SEC009");
			expect(findings[0].severity).toBe("warn");
		});

		it("should not warn on small base64 string", () => {
			const code = `const data = Buffer.from("SGVsbG8gV29ybGQ=", "base64");`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC009],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});
	});

	describe("large hex strings", () => {
		it("should warn on Buffer.from with large hex string", () => {
			const largeHex = "0".repeat(201);
			const code = `const data = Buffer.from("${largeHex}", "hex");`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC009],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
			expect(findings[0].ruleId).toBe("SEC009");
		});

		it("should not warn on small hex string", () => {
			const code = `const data = Buffer.from("48656c6c6f", "hex");`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC009],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});
	});

	describe("safe Buffer usage", () => {
		it("should not warn on Buffer.from with utf8 encoding", () => {
			const largeString = "A".repeat(300);
			const code = `const data = Buffer.from("${largeString}", "utf8");`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC009],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});

		it("should not warn on Buffer.from without encoding", () => {
			const code = `const data = Buffer.from("any string");`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC009],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});

		it("should not warn on Buffer.alloc", () => {
			const code = "const buf = Buffer.alloc(1024);";
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC009],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});
	});

	describe("threshold boundary", () => {
		it("should not warn at exactly 200 chars", () => {
			const exactly200 = "A".repeat(200);
			const code = `const data = Buffer.from("${exactly200}", "base64");`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC009],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});

		it("should warn at 201 chars", () => {
			const exactly201 = "A".repeat(201);
			const code = `const data = Buffer.from("${exactly201}", "base64");`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC009],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
		});
	});
});
