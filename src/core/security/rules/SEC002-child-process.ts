import ts from "typescript";
import { isAliasOf } from "../alias-builder.ts";
import { isMemberCall } from "../matchers.ts";
import type { Rule } from "../types.ts";

/**
 * SEC002: No child_process usage
 *
 * Detects usage of Node.js child_process module which can execute arbitrary system commands.
 *
 * **Blocked patterns:**
 * - `child_process.exec()`
 * - `child_process.spawn()`
 * - `child_process.fork()`
 * - `child_process.execFile()`
 * - All variants via aliases (ESM imports, CJS requires, namespace imports)
 *
 * **Severity:** block
 *
 * **Remediation:**
 * Remove child_process usage. Extensions cannot execute system commands.
 * Use platform APIs or request capabilities through the extension system.
 *
 * @example
 * // ❌ Blocked
 * import { exec } from 'child_process';
 * exec('rm -rf /');
 *
 * const cp = require('child_process');
 * cp.spawn('malicious');
 *
 * // ✅ Safe
 * // Use platform APIs instead
 */
export const SEC002: Rule = {
	meta: {
		id: "SEC002",
		defaultSeverity: "block",
		title: "No child_process usage",
		description:
			"Detects child_process module usage (exec, spawn, fork, execFile) that can execute system commands",
		remediation:
			"Remove child_process usage. Use platform APIs or request capabilities through the extension system.",
	},
	create: (ctx) => {
		// NOTE: Known limitation - Direct calls to renamed imports are not reliably detected
		// without full type resolution. Example: `import { exec as execute } from 'child_process'; execute('cmd');`
		// Current implementation requires namespace or member access patterns (e.g., cp.exec()).
		// Future improvement: Integrate TypeScript type checker to track renamed imports through call sites,
		// or add interprocedural analysis to track function references.

		const methods = [
			"exec",
			"spawn",
			"fork",
			"execFile",
			"execSync",
			"spawnSync",
			"execFileSync",
		];

		return {
			[ts.SyntaxKind.CallExpression]: (node: ts.Node) => {
				const callExpr = node as ts.CallExpression;

				// Check for direct method calls like exec(), spawn()
				if (ts.isIdentifier(callExpr.expression)) {
					const methodName = callExpr.expression.text;
					if (
						methods.includes(methodName) &&
						isAliasOf(methodName, "child_process", ctx.aliasMaps)
					) {
						ctx.report(
							`Blocked: child_process.${methodName}() can execute arbitrary system commands. Use platform APIs instead.`,
							node,
						);
					}
				}

				// Check for member calls like cp.exec(), childProcess.spawn()
				for (const method of methods) {
					if (
						isMemberCall(node, {
							objectIsAliasOf: "child_process",
							method,
							aliasMaps: ctx.aliasMaps,
						})
					) {
						ctx.report(
							`Blocked: child_process.${method}() can execute arbitrary system commands. Use platform APIs instead.`,
							node,
						);
						break;
					}
				}
			},
		};
	},
};
