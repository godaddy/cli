import { scanFile } from "@/core/security/engine.ts";
import type {
	AliasMaps,
	Finding,
	NodeVisitor,
	Rule,
	RuleContext,
	SecurityConfig,
} from "@/core/security/types.ts";
import ts from "typescript";
import { describe, expect, it } from "vitest";

function createSourceFile(code: string, fileName = "test.ts"): ts.SourceFile {
	return ts.createSourceFile(
		fileName,
		code,
		ts.ScriptTarget.Latest,
		true,
		ts.ScriptKind.TS,
	);
}

const mockConfig: SecurityConfig = {
	mode: "strict",
	trustedDomains: ["*.godaddy.com"],
	exclude: ["**/node_modules/**"],
};

const emptyAliasMaps: AliasMaps = {
	moduleAliases: new Map(),
	namespaceAliases: new Map(),
	namedImports: new Map(),
};

describe("AST Walker Engine", () => {
	describe("scanFile", () => {
		it("should execute single rule on simple file", () => {
			const code = `eval("malicious code");`;
			const findings: Finding[] = [];

			const evalRule: Rule = {
				meta: {
					id: "SEC001",
					defaultSeverity: "block",
					title: "No eval()",
					description: "Detects eval() usage",
					remediation: "Remove eval()",
				},
				create: (ctx: RuleContext): NodeVisitor => ({
					[ts.SyntaxKind.CallExpression]: (node: ts.Node) => {
						const call = node as ts.CallExpression;
						if (
							ts.isIdentifier(call.expression) &&
							call.expression.text === "eval"
						) {
							ctx.report("eval() is blocked", node);
						}
					},
				}),
			};

			const result = scanFile(
				"test.ts",
				code,
				[evalRule],
				mockConfig,
				emptyAliasMaps,
			);

			expect(result).toHaveLength(1);
			expect(result[0].ruleId).toBe("SEC001");
			expect(result[0].severity).toBe("block");
			expect(result[0].message).toBe("eval() is blocked");
			expect(result[0].line).toBe(1);
		});

		it("should execute multiple rules in single pass", () => {
			const code = `
        eval("code");
        new Function("code");
      `;

			const evalRule: Rule = {
				meta: {
					id: "SEC001",
					defaultSeverity: "block",
					title: "No eval()",
					description: "Detects eval()",
					remediation: "Remove eval()",
				},
				create: (ctx: RuleContext): NodeVisitor => ({
					[ts.SyntaxKind.CallExpression]: (node: ts.Node) => {
						const call = node as ts.CallExpression;
						if (
							ts.isIdentifier(call.expression) &&
							call.expression.text === "eval"
						) {
							ctx.report("eval() detected", node);
						}
					},
				}),
			};

			const functionRule: Rule = {
				meta: {
					id: "SEC002",
					defaultSeverity: "block",
					title: "No new Function()",
					description: "Detects Function constructor",
					remediation: "Remove new Function()",
				},
				create: (ctx: RuleContext): NodeVisitor => ({
					[ts.SyntaxKind.NewExpression]: (node: ts.Node) => {
						const newExpr = node as ts.NewExpression;
						if (
							ts.isIdentifier(newExpr.expression) &&
							newExpr.expression.text === "Function"
						) {
							ctx.report("new Function() detected", node);
						}
					},
				}),
			};

			const result = scanFile(
				"test.ts",
				code,
				[evalRule, functionRule],
				mockConfig,
				emptyAliasMaps,
			);

			expect(result).toHaveLength(2);
			expect(result.map((f) => f.ruleId)).toContain("SEC001");
			expect(result.map((f) => f.ruleId)).toContain("SEC002");
		});

		it("should dispatch to correct handler based on SyntaxKind", () => {
			const code = `
        const x = "string";
        const y = 42;
        function foo() {}
      `;

			let stringLiteralCount = 0;
			let numericLiteralCount = 0;
			let functionDeclCount = 0;

			const countingRule: Rule = {
				meta: {
					id: "SEC001",
					defaultSeverity: "warn",
					title: "Counter",
					description: "Counts nodes",
					remediation: "N/A",
				},
				create: (ctx: RuleContext): NodeVisitor => ({
					[ts.SyntaxKind.StringLiteral]: () => {
						stringLiteralCount++;
					},
					[ts.SyntaxKind.NumericLiteral]: () => {
						numericLiteralCount++;
					},
					[ts.SyntaxKind.FunctionDeclaration]: () => {
						functionDeclCount++;
					},
				}),
			};

			scanFile("test.ts", code, [countingRule], mockConfig, emptyAliasMaps);

			expect(stringLiteralCount).toBe(1);
			expect(numericLiteralCount).toBe(1);
			expect(functionDeclCount).toBe(1);
		});

		it("should provide accurate line and column numbers", () => {
			const code = `const x = 1;
const y = 2;
eval("code");`;

			const evalRule: Rule = {
				meta: {
					id: "SEC001",
					defaultSeverity: "block",
					title: "No eval()",
					description: "Detects eval()",
					remediation: "Remove eval()",
				},
				create: (ctx: RuleContext): NodeVisitor => ({
					[ts.SyntaxKind.CallExpression]: (node: ts.Node) => {
						const call = node as ts.CallExpression;
						if (
							ts.isIdentifier(call.expression) &&
							call.expression.text === "eval"
						) {
							ctx.report("eval() detected", node);
						}
					},
				}),
			};

			const result = scanFile(
				"test.ts",
				code,
				[evalRule],
				mockConfig,
				emptyAliasMaps,
			);

			expect(result).toHaveLength(1);
			expect(result[0].line).toBe(3);
			expect(result[0].col).toBeGreaterThan(0);
		});

		it("should call onFileStart hook if defined", () => {
			const code = "const x = 1;";
			let hookCalled = false;

			const hookRule: Rule = {
				meta: {
					id: "SEC001",
					defaultSeverity: "warn",
					title: "Hook Test",
					description: "Tests onFileStart hook",
					remediation: "N/A",
				},
				create: (): NodeVisitor => ({
					onFileStart: () => {
						hookCalled = true;
					},
				}),
			};

			scanFile("test.ts", code, [hookRule], mockConfig, emptyAliasMaps);

			expect(hookCalled).toBe(true);
		});

		it("should generate code snippet for findings", () => {
			const code = `eval("very long string that should be truncated at some point because it exceeds the maximum length");`;

			const evalRule: Rule = {
				meta: {
					id: "SEC001",
					defaultSeverity: "block",
					title: "No eval()",
					description: "Detects eval()",
					remediation: "Remove eval()",
				},
				create: (ctx: RuleContext): NodeVisitor => ({
					[ts.SyntaxKind.CallExpression]: (node: ts.Node) => {
						const call = node as ts.CallExpression;
						if (
							ts.isIdentifier(call.expression) &&
							call.expression.text === "eval"
						) {
							ctx.report("eval() detected", node);
						}
					},
				}),
			};

			const result = scanFile(
				"test.ts",
				code,
				[evalRule],
				mockConfig,
				emptyAliasMaps,
			);

			expect(result[0].snippet).toBeDefined();
			expect(result[0].snippet!.length).toBeLessThanOrEqual(80);
		});

		it("should handle files with no violations", () => {
			const code = "const x = 1; const y = 2;";

			const evalRule: Rule = {
				meta: {
					id: "SEC001",
					defaultSeverity: "block",
					title: "No eval()",
					description: "Detects eval()",
					remediation: "Remove eval()",
				},
				create: (ctx: RuleContext): NodeVisitor => ({
					[ts.SyntaxKind.CallExpression]: (node: ts.Node) => {
						const call = node as ts.CallExpression;
						if (
							ts.isIdentifier(call.expression) &&
							call.expression.text === "eval"
						) {
							ctx.report("eval() detected", node);
						}
					},
				}),
			};

			const result = scanFile(
				"test.ts",
				code,
				[evalRule],
				mockConfig,
				emptyAliasMaps,
			);

			expect(result).toHaveLength(0);
		});

		it("should pass aliasMaps to rule context", () => {
			const code = `cp.exec("command");`;
			const aliasMaps: AliasMaps = {
				moduleAliases: new Map([["child_process", new Set(["cp"])]]),
				namespaceAliases: new Map(),
				namedImports: new Map(),
			};

			let contextAliasMaps: AliasMaps | undefined;

			const childProcessRule: Rule = {
				meta: {
					id: "SEC002",
					defaultSeverity: "block",
					title: "No child_process",
					description: "Detects child_process usage",
					remediation: "Remove child_process",
				},
				create: (ctx: RuleContext): NodeVisitor => {
					contextAliasMaps = ctx.aliasMaps;
					return {};
				},
			};

			scanFile("test.ts", code, [childProcessRule], mockConfig, aliasMaps);

			expect(contextAliasMaps).toBe(aliasMaps);
			expect(contextAliasMaps?.moduleAliases.get("child_process")).toContain(
				"cp",
			);
		});

		it("should include file path in findings", () => {
			const code = `eval("code");`;

			const evalRule: Rule = {
				meta: {
					id: "SEC001",
					defaultSeverity: "block",
					title: "No eval()",
					description: "Detects eval()",
					remediation: "Remove eval()",
				},
				create: (ctx: RuleContext): NodeVisitor => ({
					[ts.SyntaxKind.CallExpression]: (node: ts.Node) => {
						const call = node as ts.CallExpression;
						if (
							ts.isIdentifier(call.expression) &&
							call.expression.text === "eval"
						) {
							ctx.report("eval() detected", node);
						}
					},
				}),
			};

			const result = scanFile(
				"src/malicious.ts",
				code,
				[evalRule],
				mockConfig,
				emptyAliasMaps,
			);

			expect(result[0].file).toBe("src/malicious.ts");
		});
	});
});
