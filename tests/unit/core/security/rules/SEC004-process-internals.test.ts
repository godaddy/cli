import { buildAliasMaps } from "@/core/security/alias-builder.ts";
import { scanFile } from "@/core/security/engine.ts";
import { SEC004 } from "@/core/security/rules/SEC004-process-internals.ts";
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

describe("SEC004: No process internals access", () => {
	describe("process.binding", () => {
		it("should detect process.binding() call", () => {
			const code = `const natives = process.binding('natives');`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC004],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
			expect(findings[0].ruleId).toBe("SEC004");
			expect(findings[0].severity).toBe("block");
		});

		it("should detect process.binding property access", () => {
			const code = "const fn = process.binding;";
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC004],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
		});
	});

	describe("process.dlopen", () => {
		it("should detect process.dlopen() call", () => {
			const code = `process.dlopen(module, 'addon.node');`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC004],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
			expect(findings[0].ruleId).toBe("SEC004");
		});

		it("should detect process.dlopen property access", () => {
			const code = "const dlopen = process.dlopen;";
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC004],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
		});
	});

	describe("safe process usage", () => {
		it("should not detect process.env", () => {
			const code = "const env = process.env.NODE_ENV;";
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC004],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});

		it("should not detect process.cwd", () => {
			const code = "const cwd = process.cwd();";
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC004],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});

		it("should not detect process.exit", () => {
			const code = "process.exit(0);";
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC004],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});
	});
});
