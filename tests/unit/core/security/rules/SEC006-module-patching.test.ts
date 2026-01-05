import { buildAliasMaps } from "@/core/security/alias-builder.ts";
import { scanFile } from "@/core/security/engine.ts";
import { SEC006 } from "@/core/security/rules/SEC006-module-patching.ts";
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

describe("SEC006: No module system patching", () => {
	describe("Module._load", () => {
		it("should detect Module._load access", () => {
			const code = `
        const Module = require('module');
        Module._load = function() {};
      `;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC006],
				mockConfig,
				aliasMaps,
			);

			expect(findings.length).toBeGreaterThan(0);
			expect(findings.some((f) => f.ruleId === "SEC006")).toBe(true);
		});
	});

	describe("Module._extensions", () => {
		it("should detect Module._extensions access", () => {
			const code = `
        const Module = require('module');
        Module._extensions['.custom'] = () => {};
      `;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC006],
				mockConfig,
				aliasMaps,
			);

			expect(findings.length).toBeGreaterThan(0);
		});
	});

	describe("Module._compile", () => {
		it("should detect Module._compile access", () => {
			const code = `
        const Module = require('module');
        const original = Module._compile;
      `;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC006],
				mockConfig,
				aliasMaps,
			);

			expect(findings.length).toBeGreaterThan(0);
		});
	});

	describe("require.extensions", () => {
		it("should detect require.extensions access", () => {
			const code = `require.extensions['.custom'] = () => {};`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC006],
				mockConfig,
				aliasMaps,
			);

			expect(findings.length).toBeGreaterThan(0);
		});
	});

	describe("safe code", () => {
		it("should not detect regular Module usage", () => {
			const code = `
        class Module {
          load() {}
        }
        const m = new Module();
      `;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC006],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});

		it("should not detect regular require calls", () => {
			const code = `const fs = require('fs');`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC006],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});
	});
});
