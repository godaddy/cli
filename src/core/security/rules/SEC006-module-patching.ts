import ts from "typescript";
import type { Rule } from "../types.ts";

/**
 * SEC006: No module system patching
 *
 * Detects attempts to patch Node.js module loading internals.
 *
 * **Blocked patterns:**
 * - `Module._load`
 * - `Module._extensions`
 * - `Module._compile`
 * - `require.extensions`
 *
 * **Severity:** block
 *
 * **Remediation:**
 * Remove module system patching. Do not modify Node.js internals.
 * Use standard module loading mechanisms.
 *
 * @example
 * ```ts
 * // ❌ Blocked
 * const Module = require('module');
 * Module._load = function() { };
 * Module._extensions['.custom'] = () => { };
 * require.extensions['.custom'] = () => { };
 *
 * // ✅ Safe
 * // Use standard imports/requires
 * ```
 */
export const SEC006: Rule = {
	meta: {
		id: "SEC006",
		defaultSeverity: "block",
		title: "No module system patching",
		description:
			"Detects attempts to patch Node.js module loading internals (Module._load, Module._extensions, Module._compile)",
		remediation:
			"Remove module system patching. Use standard module loading mechanisms.",
	},
	create: (ctx) => {
		// NOTE: Current list covers the most commonly abused Module internals for patching.
		// Future improvement: Consider adding detection for:
		// - Module._cache manipulation (clearing or replacing cached modules)
		// - Module._pathCache, Module._findPath (path resolution manipulation)
		// - Module.prototype modifications (monkey-patching module prototype)
		// Monitor ecosystem for new module patching patterns.

		const moduleInternals = [
			"_load",
			"_extensions",
			"_compile",
			"_resolveFilename",
		];

		return {
			[ts.SyntaxKind.PropertyAccessExpression]: (node: ts.Node) => {
				const propAccess = node as ts.PropertyAccessExpression;

				// Check for Module._load, Module._extensions, etc.
				if (
					ts.isIdentifier(propAccess.expression) &&
					propAccess.expression.text === "Module" &&
					ts.isIdentifier(propAccess.name) &&
					moduleInternals.includes(propAccess.name.text)
				) {
					ctx.report(
						`Blocked: Module.${propAccess.name.text} is an internal Node.js API. Do not patch module loading.`,
						node,
					);
				}

				// Check for require.extensions
				if (
					ts.isIdentifier(propAccess.expression) &&
					propAccess.expression.text === "require" &&
					ts.isIdentifier(propAccess.name) &&
					propAccess.name.text === "extensions"
				) {
					ctx.report(
						"Blocked: require.extensions patches module loading. Use standard module loading instead.",
						node,
					);
				}
			},
		};
	},
};
