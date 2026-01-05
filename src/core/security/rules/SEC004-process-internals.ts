import ts from "typescript";
import { isProcessProperty } from "../matchers.ts";
import type { Rule } from "../types.ts";

/**
 * SEC004: No process internals access
 *
 * Detects access to Node.js internal process APIs that bypass normal module loading.
 *
 * **Blocked patterns:**
 * - `process.binding()`
 * - `process.dlopen()`
 *
 * **Severity:** block
 *
 * **Remediation:**
 * Remove process.binding() and process.dlopen() usage. These are internal Node.js APIs
 * not intended for application code.
 *
 * @example
 * // ❌ Blocked
 * const natives = process.binding('natives');
 * process.dlopen(module, 'addon.node');
 *
 * // ✅ Safe
 * // Use standard require/import
 */
export const SEC004: Rule = {
	meta: {
		id: "SEC004",
		defaultSeverity: "block",
		title: "No process internals access",
		description:
			"Detects access to internal Node.js APIs (process.binding, process.dlopen) that bypass normal module loading",
		remediation:
			"Remove process.binding() and process.dlopen(). Use standard require/import instead.",
	},
	create: (ctx) => {
		// NOTE: Current scope covers process.binding and process.dlopen as the most dangerous internals.
		// Future improvement: Consider adding other internal process properties that could pose security risks:
		// - process._debugProcess, process._debugEnd (debugging internals)
		// - process._rawDebug (low-level debugging)
		// Monitor Node.js internal API changes for new dangerous properties.

		return {
			[ts.SyntaxKind.PropertyAccessExpression]: (node: ts.Node) => {
				if (isProcessProperty(node, "binding")) {
					ctx.report(
						"Blocked: process.binding() is an internal Node.js API. Use standard require/import instead.",
						node,
					);
				}
				if (isProcessProperty(node, "dlopen")) {
					ctx.report(
						"Blocked: process.dlopen() is an internal Node.js API. Use standard require/import instead.",
						node,
					);
				}
			},
		};
	},
};
