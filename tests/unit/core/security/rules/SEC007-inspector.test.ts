import { buildAliasMaps } from "@/core/security/alias-builder.ts";
import { scanFile } from "@/core/security/engine.ts";
import { SEC007 } from "@/core/security/rules/SEC007-inspector.ts";
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

describe("SEC007: No inspector module", () => {
	describe("ESM imports", () => {
		it("should detect inspector import", () => {
			const code = `import inspector from 'inspector';`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC007],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
			expect(findings[0].ruleId).toBe("SEC007");
			expect(findings[0].severity).toBe("block");
		});

		it("should detect inspector namespace import", () => {
			const code = `import * as inspector from 'inspector';`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC007],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
		});

		it("should detect inspector named import", () => {
			const code = `import { open } from 'inspector';`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC007],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
		});
	});

	describe("CJS require", () => {
		it("should detect require('inspector')", () => {
			const code = `const inspector = require('inspector');`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC007],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
		});

		it("should detect destructured require", () => {
			const code = `const { open } = require('inspector');`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC007],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
		});
	});

	describe("safe code", () => {
		it("should not detect custom inspector object", () => {
			const code = `
        const inspector = { open: () => {} };
        inspector.open();
      `;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC007],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});
	});
});
