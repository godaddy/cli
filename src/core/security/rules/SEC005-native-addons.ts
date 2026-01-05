import ts from "typescript";
import { isImportOf, isRequireOf } from "../matchers.ts";
import type { Rule } from "../types.ts";

/**
 * SEC005: No native addons
 *
 * Detects usage of native Node.js addons (.node files) and native binding libraries.
 *
 * **Blocked patterns:**
 * - `require('*.node')` - direct .node file loading
 * - `import 'node-gyp-build'` - native addon builder
 * - `import 'ffi-napi'` - foreign function interface
 * - `import 'ref-napi'` - native memory access
 *
 * **Severity:** block
 *
 * **Remediation:**
 * Remove native addon usage. Extensions must use pure JavaScript/TypeScript.
 * Request platform capabilities if native functionality is needed.
 *
 * @example
 * // ❌ Blocked
 * require('./addon.node');
 * import binding from 'node-gyp-build';
 * import ffi from 'ffi-napi';
 *
 * // ✅ Safe
 * // Use pure JS/TS libraries
 */
export const SEC005: Rule = {
	meta: {
		id: "SEC005",
		defaultSeverity: "block",
		title: "No native addons",
		description:
			"Detects native Node.js addons (.node files) and native binding libraries (node-gyp-build, ffi-napi, ref-napi)",
		remediation:
			"Remove native addon usage. Use pure JavaScript/TypeScript or request platform capabilities.",
	},
	create: (ctx) => {
		// TODO: Expand this list as new native binding libraries emerge. Monitor npm ecosystem for:
		// - prebuild-install, node-pre-gyp (native addon installers)
		// - koffi, nbind, napi-rs (alternative FFI/binding systems)
		// - Platform-specific native module loaders
		// Future improvement: Consider maintaining this list in a separate configuration file.

		const nativeLibs = ["node-gyp-build", "ffi-napi", "ref-napi", "bindings"];

		return {
			[ts.SyntaxKind.ImportDeclaration]: (node: ts.Node) => {
				for (const lib of nativeLibs) {
					if (isImportOf(node, lib)) {
						ctx.report(
							`Blocked: '${lib}' is a native binding library. Extensions must use pure JavaScript/TypeScript.`,
							node,
						);
						break;
					}
				}
			},
			[ts.SyntaxKind.CallExpression]: (node: ts.Node) => {
				// Check for require('*.node')
				if (isRequireOf(node, /\.node$/)) {
					ctx.report(
						"Blocked: require('*.node') loads native addons. Extensions must use pure JavaScript/TypeScript.",
						node,
					);
				}

				// Check for require of native binding libraries
				for (const lib of nativeLibs) {
					if (isRequireOf(node, new RegExp(`^${lib}$`))) {
						ctx.report(
							`Blocked: require('${lib}') loads a native binding library. Extensions must use pure JavaScript/TypeScript.`,
							node,
						);
						break;
					}
				}
			},
		};
	},
};
