import ts from "typescript";
import { isCallToGlobal, isNewExpressionOf } from "../matchers.ts";
import type { Rule } from "../types.ts";

/**
 * SEC001: No eval() or Function constructor
 *
 * Detects dangerous code evaluation patterns that allow arbitrary code execution.
 *
 * **Blocked patterns:**
 * - `eval()` calls
 * - `new Function()` constructor
 *
 * **Severity:** block
 *
 * **Remediation:**
 * Remove eval() and Function constructor usage. Use safer alternatives like
 * JSON.parse() for data, or refactor to avoid dynamic code execution.
 *
 * @example
 * // ❌ Blocked
 * eval("console.log('danger')");
 * new Function("return 1 + 1")();
 *
 * // ✅ Safe
 * JSON.parse('{"key": "value"}');
 */
export const SEC001: Rule = {
	meta: {
		id: "SEC001",
		defaultSeverity: "block",
		title: "No eval() or Function constructor",
		description:
			"Detects eval() calls and Function constructor usage that allow arbitrary code execution",
		remediation:
			"Remove eval() and new Function(). Use JSON.parse() for data or refactor to avoid dynamic code execution.",
	},
	create: (ctx) => {
		// NOTE: Known limitation - Cannot distinguish custom Function classes from global Function
		// without full type checking. This may cause false positives if users define their own
		// Function class. This is an acceptable trade-off for security scanning without type resolution.
		// Future improvement: Consider integrating TypeScript type checker for more accurate detection.

		return {
			[ts.SyntaxKind.CallExpression]: (node: ts.Node) => {
				if (isCallToGlobal(node, "eval")) {
					ctx.report(
						"Blocked: eval() allows arbitrary code execution. Use JSON.parse() for data or refactor code.",
						node,
					);
				}
			},
			[ts.SyntaxKind.NewExpression]: (node: ts.Node) => {
				if (isNewExpressionOf(node, "Function")) {
					ctx.report(
						"Blocked: new Function() allows arbitrary code execution. Use regular function declarations instead.",
						node,
					);
				}
			},
		};
	},
};
