import { buildAliasMaps } from "@/core/security/alias-builder.ts";
import { scanFile } from "@/core/security/engine.ts";
import { SEC008 } from "@/core/security/rules/SEC008-external-urls.ts";
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
	trustedDomains: ["*.godaddy.com", "localhost", "127.0.0.1"],
	exclude: [],
};

describe("SEC008: External URL detection", () => {
	describe("untrusted URLs", () => {
		it("should warn on external http URL", () => {
			const code = `const url = "http://evil.com/api";`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC008],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
			expect(findings[0].ruleId).toBe("SEC008");
			expect(findings[0].severity).toBe("warn");
		});

		it("should warn on external https URL", () => {
			const code = `fetch("https://malicious.org/data");`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC008],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
			expect(findings[0].ruleId).toBe("SEC008");
		});

		it("should warn on URL in template literal", () => {
			const code = "const url = `https://external.com/api/${id}`;";
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC008],
				mockConfig,
				aliasMaps,
			);

			expect(findings.length).toBeGreaterThan(0);
		});
	});

	describe("trusted domains", () => {
		it("should not warn on GoDaddy domain", () => {
			const code = `const url = "https://api.godaddy.com/v1/data";`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC008],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});

		it("should not warn on GoDaddy subdomain", () => {
			const code = `const url = "https://internal.godaddy.com/api";`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC008],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});

		it("should not warn on localhost", () => {
			const code = `const url = "http://localhost:3000";`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC008],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});

		it("should not warn on 127.0.0.1", () => {
			const code = `const url = "http://127.0.0.1:8080";`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC008],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});
	});

	describe("non-URL strings", () => {
		it("should not warn on regular strings", () => {
			const code = `const message = "Hello world";`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC008],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});

		it("should not warn on file paths", () => {
			const code = `const path = "/path/to/file.txt";`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC008],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});

		it("should not warn on relative URLs", () => {
			const code = `const path = "/api/endpoint";`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC008],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});
	});

	describe("multiple URLs", () => {
		it("should detect multiple untrusted URLs", () => {
			const code = `
        const url1 = "https://evil.com";
        const url2 = "http://malicious.org";
        const url3 = "https://api.godaddy.com"; // trusted
      `;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC008],
				mockConfig,
				aliasMaps,
			);

			expect(findings.length).toBe(2);
			expect(findings.every((f) => f.ruleId === "SEC008")).toBe(true);
		});
	});
});
