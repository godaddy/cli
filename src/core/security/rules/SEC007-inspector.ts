import ts from "typescript";
import { isImportOf, isRequireOf } from "../matchers.ts";
import type { Rule } from "../types.ts";

/**
 * SEC007: No inspector module
 *
 * Detects usage of Node.js inspector module which provides debugging capabilities
 * that could be exploited.
 *
 * **Blocked patterns:**
 * - `import 'inspector'`
 * - `require('inspector')`
 * - `inspector.open()`
 *
 * **Severity:** block
 *
 * **Remediation:**
 * Remove inspector module usage. Debugging should be done through standard tools,
 * not programmatically within extensions.
 *
 * @example
 * // ❌ Blocked
 * import inspector from 'inspector';
 * inspector.open();
 *
 * const insp = require('inspector');
 * insp.open();
 *
 * // ✅ Safe
 * // Use standard debugging tools
 */
export const SEC007: Rule = {
	meta: {
		id: "SEC007",
		defaultSeverity: "block",
		title: "No inspector module",
		description:
			"Detects inspector module usage which provides programmatic debugging capabilities",
		remediation:
			"Remove inspector module usage. Use standard debugging tools instead.",
	},
	create: (ctx) => {
		return {
			[ts.SyntaxKind.ImportDeclaration]: (node: ts.Node) => {
				if (isImportOf(node, "inspector")) {
					ctx.report(
						"Blocked: 'inspector' module provides programmatic debugging. Use standard debugging tools instead.",
						node,
					);
				}
			},
			[ts.SyntaxKind.CallExpression]: (node: ts.Node) => {
				if (isRequireOf(node, /^inspector$/)) {
					ctx.report(
						"Blocked: require('inspector') provides programmatic debugging. Use standard debugging tools instead.",
						node,
					);
				}
			},
		};
	},
};
