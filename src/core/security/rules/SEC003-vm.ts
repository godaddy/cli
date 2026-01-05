import ts from "typescript";
import { isAliasOf } from "../alias-builder.ts";
import { isMemberCall } from "../matchers.ts";
import type { Rule } from "../types.ts";

/**
 * SEC003: No vm module usage
 *
 * Detects usage of Node.js vm module which can execute code in isolated contexts
 * but still poses security risks.
 *
 * **Blocked patterns:**
 * - `vm.runInNewContext()`
 * - `vm.runInContext()`
 * - `vm.runInThisContext()`
 * - `new vm.Script()`
 * - All variants via aliases
 *
 * **Severity:** block
 *
 * **Remediation:**
 * Remove vm module usage. Use safe data transformation instead of code execution.
 *
 * @example
 * // ❌ Blocked
 * import * as vm from 'vm';
 * vm.runInNewContext('malicious code');
 * new vm.Script('code');
 *
 * // ✅ Safe
 * // Use data transformation libraries
 */
export const SEC003: Rule = {
	meta: {
		id: "SEC003",
		defaultSeverity: "block",
		title: "No vm module usage",
		description:
			"Detects vm module usage (runInNewContext, runInContext, Script) that can execute code in isolated contexts",
		remediation:
			"Remove vm module usage. Use safe data transformation instead of code execution.",
	},
	create: (ctx) => {
		// NOTE: Detection of heavily transformed or complex alias usage may be limited.
		// Current implementation covers common use cases (namespace imports, member access).
		// Future improvement: Add detection for vm methods passed as callbacks or stored in variables.

		const methods = [
			"runInNewContext",
			"runInContext",
			"runInThisContext",
			"createContext",
		];

		return {
			[ts.SyntaxKind.CallExpression]: (node: ts.Node) => {
				const callExpr = node as ts.CallExpression;

				// Check for direct method calls like runInNewContext()
				if (ts.isIdentifier(callExpr.expression)) {
					const methodName = callExpr.expression.text;
					if (
						methods.includes(methodName) &&
						isAliasOf(methodName, "vm", ctx.aliasMaps)
					) {
						ctx.report(
							`Blocked: vm.${methodName}() can execute arbitrary code. Use safe data transformation instead.`,
							node,
						);
					}
				}

				// Check for member calls like vm.runInNewContext()
				for (const method of methods) {
					if (
						isMemberCall(node, {
							objectIsAliasOf: "vm",
							method,
							aliasMaps: ctx.aliasMaps,
						})
					) {
						ctx.report(
							`Blocked: vm.${method}() can execute arbitrary code. Use safe data transformation instead.`,
							node,
						);
						break;
					}
				}
			},
			[ts.SyntaxKind.NewExpression]: (node: ts.Node) => {
				const newExpr = node as ts.NewExpression;

				// Check for new vm.Script()
				if (ts.isPropertyAccessExpression(newExpr.expression)) {
					const objExpr = newExpr.expression.expression;
					const propName = newExpr.expression.name;

					if (
						ts.isIdentifier(objExpr) &&
						ts.isIdentifier(propName) &&
						propName.text === "Script" &&
						isAliasOf(objExpr.text, "vm", ctx.aliasMaps)
					) {
						ctx.report(
							"Blocked: new vm.Script() can compile and execute arbitrary code. Use safe data transformation instead.",
							node,
						);
					}
				}
			},
		};
	},
};
