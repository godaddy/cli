import { buildAliasMaps } from "@/core/security/alias-builder.ts";
import { scanFile } from "@/core/security/engine.ts";
import { SEC003 } from "@/core/security/rules/SEC003-vm.ts";
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

describe("SEC003: No vm module usage", () => {
	describe("vm.runInNewContext", () => {
		it("should detect namespace import usage", () => {
			const code = `
        import * as vm from 'vm';
        vm.runInNewContext('malicious code');
      `;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC003],
				mockConfig,
				aliasMaps,
			);

			expect(findings.length).toBeGreaterThan(0);
			expect(findings[0].ruleId).toBe("SEC003");
			expect(findings[0].severity).toBe("block");
		});

		it("should detect named import usage", () => {
			const code = `
        import { runInNewContext } from 'vm';
        runInNewContext('code');
      `;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC003],
				mockConfig,
				aliasMaps,
			);

			expect(findings.length).toBeGreaterThan(0);
		});
	});

	describe("vm.runInContext", () => {
		it("should detect runInContext", () => {
			const code = `
        import { runInContext } from 'vm';
        const context = {};
        runInContext('code', context);
      `;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC003],
				mockConfig,
				aliasMaps,
			);

			expect(findings.length).toBeGreaterThan(0);
		});
	});

	describe("vm.Script", () => {
		it("should detect new vm.Script()", () => {
			const code = `
        import * as vm from 'vm';
        const script = new vm.Script('code');
      `;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC003],
				mockConfig,
				aliasMaps,
			);

			expect(findings.length).toBeGreaterThan(0);
		});
	});

	describe("CJS require", () => {
		it("should detect require vm module", () => {
			const code = `
        const vm = require('vm');
        vm.runInNewContext('code');
      `;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC003],
				mockConfig,
				aliasMaps,
			);

			expect(findings.length).toBeGreaterThan(0);
		});
	});

	describe("safe code", () => {
		it("should not detect custom vm object", () => {
			const code = `
        const vm = { runInNewContext: () => {} };
        vm.runInNewContext();
      `;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC003],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});
	});
});
