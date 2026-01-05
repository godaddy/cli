import { buildAliasMaps } from "@/core/security/alias-builder.ts";
import { scanFile } from "@/core/security/engine.ts";
import { SEC001 } from "@/core/security/rules/SEC001-eval.ts";
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

describe("SEC001: No eval() or Function constructor", () => {
	describe("eval() detection", () => {
		it("should detect direct eval() call", () => {
			const code = `eval("malicious code");`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC001],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
			expect(findings[0].ruleId).toBe("SEC001");
			expect(findings[0].severity).toBe("block");
		});

		it("should detect eval() with variable argument", () => {
			const code = `
        const userInput = getUserInput();
        eval(userInput);
      `;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC001],
				mockConfig,
				aliasMaps,
			);

			expect(findings.length).toBeGreaterThan(0);
			expect(findings.some((f) => f.ruleId === "SEC001")).toBe(true);
		});

		it("should not detect safe function named eval", () => {
			const code = `
        function myEval(data: string) {
          return JSON.parse(data);
        }
        myEval("{}");
      `;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC001],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});
	});

	describe("Function constructor detection", () => {
		it("should detect new Function() call", () => {
			const code = `const fn = new Function("return 1 + 1");`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC001],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
			expect(findings[0].ruleId).toBe("SEC001");
			expect(findings[0].severity).toBe("block");
		});

		it("should detect new Function() with multiple arguments", () => {
			const code = `const fn = new Function("a", "b", "return a + b");`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC001],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
			expect(findings[0].ruleId).toBe("SEC001");
		});

		it("should detect new Function even with custom class (cannot distinguish)", () => {
			const code = `
        class Function {
          constructor(code: string) {}
        }
        const fn = new Function("code");
      `;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC001],
				mockConfig,
				aliasMaps,
			);

			// Note: We cannot distinguish custom Function classes from global Function
			// without full type checking, so this will be flagged
			expect(findings).toHaveLength(1);
		});
	});

	describe("safe alternatives", () => {
		it("should not detect JSON.parse", () => {
			const code = `const data = JSON.parse('{"key": "value"}');`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC001],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});

		it("should not detect regular function declarations", () => {
			const code = `
        function add(a: number, b: number) {
          return a + b;
        }
        const result = add(1, 2);
      `;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC001],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});
	});

	describe("edge cases", () => {
		it("should detect multiple violations in same file", () => {
			const code = `
        eval("code1");
        new Function("code2");
        eval("code3");
      `;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC001],
				mockConfig,
				aliasMaps,
			);

			expect(findings.length).toBeGreaterThanOrEqual(3);
		});
	});
});
