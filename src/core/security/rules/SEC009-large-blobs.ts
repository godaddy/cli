import ts from "typescript";
import { isBufferFromCall } from "../matchers.ts";
import type { Rule } from "../types.ts";

/**
 * SEC009: Large encoded blob detection (warning)
 *
 * Detects large base64 or hex encoded data in Buffer.from() calls.
 * Large encoded blobs can hide malicious payloads or executables.
 *
 * **Warned patterns:**
 * - `Buffer.from(str, 'base64')` where str.length > 200
 * - `Buffer.from(str, 'hex')` where str.length > 200
 *
 * **Severity:** warn
 *
 * **Remediation:**
 * Review large encoded data. If legitimate, load from external files.
 * Do not embed large binary data in source code.
 *
 * @example
 * // ⚠️ Warning
 * const data = Buffer.from('VGhpcyBpcyBhIHZlcnkgbG9uZyBiYXNlNjQgc3RyaW5nLi4u' + '...', 'base64');
 *
 * // ✅ No warning
 * const small = Buffer.from('SGVsbG8=', 'base64'); // < 200 chars
 * const data = fs.readFileSync('data.bin'); // Load from file
 */
export const SEC009: Rule = {
	meta: {
		id: "SEC009",
		defaultSeverity: "warn",
		title: "Large encoded blob detection",
		description:
			"Warns about large base64/hex encoded data in Buffer.from() calls (>200 chars) that could hide payloads",
		remediation:
			"Review large encoded data. Load from external files instead of embedding in source.",
	},
	create: (ctx) => {
		// TODO: Consider making SIZE_THRESHOLD configurable via SecurityConfig instead of hardcoded constant.
		// NOTE: Current implementation only checks Buffer.from(). Other buffer creation methods exist:
		// - Buffer.alloc() with subsequent writes
		// - new Buffer() (deprecated but still used)
		// - Uint8Array, ArrayBuffer conversions
		// Future improvement: Detect large binary data regardless of encoding method.

		const SIZE_THRESHOLD = 200;

		return {
			[ts.SyntaxKind.CallExpression]: (node: ts.Node) => {
				const callExpr = node as ts.CallExpression;

				// Check for base64 encoded blobs
				if (isBufferFromCall(node, "base64")) {
					const firstArg = callExpr.arguments[0];
					if (
						ts.isStringLiteral(firstArg) &&
						firstArg.text.length > SIZE_THRESHOLD
					) {
						ctx.report(
							`Warning: Large base64-encoded blob (${firstArg.text.length} chars) detected. Load from external file instead.`,
							node,
						);
					}
				}

				// Check for hex encoded blobs
				if (isBufferFromCall(node, "hex")) {
					const firstArg = callExpr.arguments[0];
					if (
						ts.isStringLiteral(firstArg) &&
						firstArg.text.length > SIZE_THRESHOLD
					) {
						ctx.report(
							`Warning: Large hex-encoded blob (${firstArg.text.length} chars) detected. Load from external file instead.`,
							node,
						);
					}
				}
			},
		};
	},
};
