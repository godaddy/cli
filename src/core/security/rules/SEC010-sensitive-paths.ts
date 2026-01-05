import ts from "typescript";
import { matchesSensitivePath } from "../matchers.ts";
import type { Rule } from "../types.ts";

/**
 * SEC010: Sensitive file path detection (warning)
 *
 * Detects string literals containing paths to sensitive system files or directories.
 *
 * **Warned patterns:**
 * - `~/.ssh` - SSH keys
 * - `/etc/passwd` - User accounts
 * - `/etc/shadow` - Password hashes
 * - `/var/run/secrets` - Kubernetes secrets
 * - `.env` files
 *
 * **Severity:** warn
 *
 * **Remediation:**
 * Review sensitive path access. Extensions should not access system files directly.
 * Use platform APIs for configuration and secrets.
 *
 * @example
 * // ⚠️ Warning
 * fs.readFileSync('~/.ssh/id_rsa');
 * const passwd = '/etc/passwd';
 * fs.readFileSync('/var/run/secrets/token');
 *
 * // ✅ No warning
 * const config = await platform.getConfig();
 */
export const SEC010: Rule = {
	meta: {
		id: "SEC010",
		defaultSeverity: "warn",
		title: "Sensitive file path detection",
		description:
			"Warns about string literals containing paths to sensitive files (~/.ssh, /etc/passwd, /var/run/secrets)",
		remediation:
			"Remove sensitive path access. Use platform APIs for configuration and secrets.",
	},
	create: (ctx) => {
		return {
			[ts.SyntaxKind.StringLiteral]: (node: ts.Node) => {
				const literal = node as ts.StringLiteral;
				if (matchesSensitivePath(literal.text)) {
					ctx.report(
						`Warning: Sensitive file path '${literal.text}' detected. Use platform APIs for configuration and secrets.`,
						node,
					);
				}
			},
		};
	},
};
