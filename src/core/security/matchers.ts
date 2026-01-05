import * as ts from "typescript";
import { isAliasOf } from "./alias-builder.ts";
import type { AliasMaps } from "./types.ts";

/**
 * Check if a node is an Identifier with a specific name.
 *
 * @param node - AST node to check
 * @param name - Expected identifier name
 * @returns True if node is an Identifier matching the specified name
 *
 * @example
 * ```ts
 * isIdentifier(node, 'eval') // true for identifier named 'eval'
 * ```
 */
export function isIdentifier(node: ts.Node, name: string): boolean {
	return ts.isIdentifier(node) && node.text === name;
}

/**
 * Check if a node is a CallExpression to a global function.
 *
 * Detects patterns like: `eval(code)`, `require('module')`
 *
 * @param node - AST node to check
 * @param name - Expected global function name
 * @returns True if node is a call to the specified global
 *
 * @example
 * ```ts
 * isCallToGlobal(node, 'eval') // true for eval(...)
 * isCallToGlobal(node, 'require') // true for require(...)
 * ```
 */
export function isCallToGlobal(node: ts.Node, name: string): boolean {
	return (
		ts.isCallExpression(node) &&
		ts.isIdentifier(node.expression) &&
		node.expression.text === name
	);
}

/**
 * Check if a node is a NewExpression with a specific constructor.
 *
 * Detects patterns like: `new Function(...)`, `new vm.Script(...)`
 *
 * @param node - AST node to check
 * @param name - Expected constructor name
 * @returns True if node is a new expression with the specified constructor
 *
 * @example
 * ```ts
 * isNewExpressionOf(node, 'Function') // true for new Function(...)
 * ```
 */
export function isNewExpressionOf(node: ts.Node, name: string): boolean {
	return (
		ts.isNewExpression(node) &&
		ts.isIdentifier(node.expression) &&
		node.expression.text === name
	);
}

/**
 * Options for member call detection.
 */
export interface MemberCallOptions {
	/** Module name to check object against (e.g., 'child_process') */
	objectIsAliasOf: string;
	/** Method name to match (e.g., 'exec', 'spawn') */
	method: string;
	/** Alias maps for module tracking */
	aliasMaps: AliasMaps;
}

/**
 * Check if a node is a member call like `cp.exec()` where `cp` is an alias of a module.
 *
 * Detects patterns like: `cp.exec()`, `vm.runInContext()` where the object
 * is imported/required from a specific module.
 *
 * @param node - AST node to check
 * @param options - Configuration for member call detection
 * @returns True if node matches the member call pattern
 *
 * @example
 * ```ts
 * // Given: import cp from 'child_process'; cp.exec('ls');
 * isMemberCall(node, {
 *   objectIsAliasOf: 'child_process',
 *   method: 'exec',
 *   aliasMaps
 * }) // true
 * ```
 */
export function isMemberCall(
	node: ts.Node,
	options: MemberCallOptions,
): boolean {
	if (!ts.isCallExpression(node)) {
		return false;
	}

	const { expression } = node;
	if (!ts.isPropertyAccessExpression(expression)) {
		return false;
	}

	// Check if method name matches
	if (!ts.isIdentifier(expression.name)) {
		return false;
	}
	if (expression.name.text !== options.method) {
		return false;
	}

	// Check if object is an alias of the target module
	const objectExpr = expression.expression;
	if (!ts.isIdentifier(objectExpr)) {
		return false;
	}

	return isAliasOf(objectExpr.text, options.objectIsAliasOf, options.aliasMaps);
}

/**
 * Check if a node is a MemberExpression accessing a specific process property.
 *
 * Detects patterns like: `process.binding`, `process.dlopen`
 *
 * @param node - AST node to check
 * @param property - Expected property name on process object
 * @returns True if node accesses the specified process property
 *
 * @example
 * ```ts
 * isProcessProperty(node, 'binding') // true for process.binding
 * isProcessProperty(node, 'dlopen') // true for process.dlopen
 * ```
 */
export function isProcessProperty(node: ts.Node, property: string): boolean {
	return (
		ts.isPropertyAccessExpression(node) &&
		ts.isIdentifier(node.expression) &&
		node.expression.text === "process" &&
		ts.isIdentifier(node.name) &&
		node.name.text === property
	);
}

/**
 * Check if a node is a require() call with a module name matching a pattern.
 *
 * Detects patterns like: `require('inspector')`, `require('*.node')`
 *
 * @param node - AST node to check
 * @param pattern - Regex pattern to match against module name
 * @returns True if node is a require call with matching module
 *
 * @example
 * ```ts
 * isRequireOf(node, /\.node$/) // true for require('addon.node')
 * isRequireOf(node, /^inspector$/) // true for require('inspector')
 * ```
 */
export function isRequireOf(node: ts.Node, pattern: RegExp): boolean {
	if (!ts.isCallExpression(node)) {
		return false;
	}

	// Check if it's a require() call
	if (!ts.isIdentifier(node.expression) || node.expression.text !== "require") {
		return false;
	}

	// Check first argument is a string literal
	const args = node.arguments;
	if (args.length < 1 || !ts.isStringLiteral(args[0])) {
		return false;
	}

	const moduleName = args[0].text;
	return pattern.test(moduleName);
}

/**
 * Check if a node is an ImportDeclaration for a specific module.
 *
 * Detects patterns like: `import ... from 'inspector'`
 *
 * @param node - AST node to check
 * @param moduleName - Expected module name
 * @returns True if node imports the specified module
 *
 * @example
 * ```ts
 * isImportOf(node, 'inspector') // true for import ... from 'inspector'
 * ```
 */
export function isImportOf(node: ts.Node, moduleName: string): boolean {
	return (
		ts.isImportDeclaration(node) &&
		ts.isStringLiteral(node.moduleSpecifier) &&
		node.moduleSpecifier.text === moduleName
	);
}

/**
 * Extract string value from a StringLiteral or simple TemplateLiteral.
 *
 * Only handles simple cases without template substitutions.
 *
 * @param node - AST node to extract string from
 * @returns String value if extractable, null otherwise
 *
 * @example
 * ```ts
 * getStringLiteralValue(node) // 'hello' for "hello" or `hello`
 * getStringLiteralValue(node) // null for `hello ${world}`
 * ```
 */
export function getStringLiteralValue(node: ts.Node): string | null {
	// Handle regular string literals: "hello" or 'hello'
	if (ts.isStringLiteral(node)) {
		return node.text;
	}

	// Handle simple template literals without substitutions: `hello`
	if (node.kind === ts.SyntaxKind.NoSubstitutionTemplateLiteral) {
		return (node as ts.NoSubstitutionTemplateLiteral).text;
	}

	// Cannot extract from complex template literals or other node types
	return null;
}

/**
 * Check if a node is a Buffer.from() call, optionally with a specific encoding.
 *
 * Detects patterns like: `Buffer.from(str, 'base64')`, `Buffer.from(str, 'hex')`
 *
 * @param node - AST node to check
 * @param encoding - Optional encoding to match ('base64' or 'hex')
 * @returns True if node is a Buffer.from call with optional encoding match
 *
 * @example
 * ```ts
 * isBufferFromCall(node) // true for any Buffer.from(...)
 * isBufferFromCall(node, 'base64') // true only for Buffer.from(..., 'base64')
 * ```
 */
export function isBufferFromCall(
	node: ts.Node,
	encoding?: "base64" | "hex",
): boolean {
	if (!ts.isCallExpression(node)) {
		return false;
	}

	const { expression } = node;
	if (!ts.isPropertyAccessExpression(expression)) {
		return false;
	}

	// Check if it's Buffer.from
	if (
		!ts.isIdentifier(expression.expression) ||
		expression.expression.text !== "Buffer"
	) {
		return false;
	}

	if (!ts.isIdentifier(expression.name) || expression.name.text !== "from") {
		return false;
	}

	// If encoding is specified, check the second argument
	if (encoding) {
		const args = node.arguments;
		if (args.length < 2 || !ts.isStringLiteral(args[1])) {
			return false;
		}
		return args[1].text === encoding;
	}

	return true;
}

/**
 * Check if a string matches a URL pattern (http or https).
 *
 * @param str - String to check
 * @returns True if string contains an http(s) URL
 *
 * @example
 * ```ts
 * matchesUrl('https://example.com') // true
 * matchesUrl('http://api.evil.com/data') // true
 * matchesUrl('file:///etc/passwd') // false
 * ```
 */
export function matchesUrl(str: string): boolean {
	const urlPattern = /https?:\/\//;
	return urlPattern.test(str);
}

/**
 * Check if a string references a sensitive file path.
 *
 * Detects patterns like: ~/.ssh, /etc/passwd, /var/run/secrets
 *
 * @param str - String to check
 * @returns True if string contains a sensitive path pattern
 *
 * @example
 * ```ts
 * matchesSensitivePath('~/.ssh/id_rsa') // true
 * matchesSensitivePath('/etc/passwd') // true
 * matchesSensitivePath('/var/run/secrets/token') // true
 * matchesSensitivePath('./config.json') // false
 * ```
 */
export function matchesSensitivePath(str: string): boolean {
	const sensitivePatterns = [
		/~\/\.ssh/,
		/\/etc\/passwd/,
		/\/etc\/shadow/,
		/\/var\/run\/secrets/,
	];

	return sensitivePatterns.some((pattern) => pattern.test(str));
}
