import * as ts from "typescript";
import type { AliasMaps } from "./types.ts";

/**
 * Build alias maps from a TypeScript source file by traversing its AST.
 *
 * Performs a single-pass traversal to collect:
 * - ESM imports (default, namespace, named)
 * - CommonJS require statements
 * - Dynamic import() calls (string literals only)
 *
 * @param sourceFile - TypeScript source file to analyze
 * @returns AliasMaps containing module aliases, namespace aliases, and named imports
 *
 * @example
 * ```ts
 * const sourceFile = ts.createSourceFile('test.ts', code, ts.ScriptTarget.Latest);
 * const maps = buildAliasMaps(sourceFile);
 * // maps.moduleAliases.get('fs') -> Set(['fs', 'fileSystem'])
 * ```
 */
export function buildAliasMaps(sourceFile: ts.SourceFile): AliasMaps {
	const moduleAliases = new Map<string, Set<string>>();
	const namespaceAliases = new Map<string, string>();
	const namedImports = new Map<string, Map<string, string>>();

	function visit(node: ts.Node): void {
		// Handle ESM imports
		if (ts.isImportDeclaration(node)) {
			handleImportDeclaration(node);
		}

		// Handle CommonJS requires in variable declarations
		if (ts.isVariableStatement(node)) {
			handleVariableStatement(node);
		}

		// Handle dynamic imports
		if (ts.isCallExpression(node)) {
			handleDynamicImport(node);
		}

		ts.forEachChild(node, visit);
	}

	function handleImportDeclaration(node: ts.ImportDeclaration): void {
		const moduleSpecifier = node.moduleSpecifier;
		if (!ts.isStringLiteral(moduleSpecifier)) {
			return;
		}

		const moduleName = moduleSpecifier.text;
		const importClause = node.importClause;

		if (!importClause) {
			// Side-effect import: import 'polyfill'
			return;
		}

		// Default import: import fs from 'fs'
		if (importClause.name) {
			const localName = importClause.name.text;
			addModuleAlias(moduleName, localName);
		}

		// Named bindings
		if (importClause.namedBindings) {
			const bindings = importClause.namedBindings;

			// Namespace import: import * as VM from 'vm'
			if (ts.isNamespaceImport(bindings)) {
				namespaceAliases.set(moduleName, bindings.name.text);
			}

			// Named imports: import { exec, spawn as sp } from 'child_process'
			if (ts.isNamedImports(bindings)) {
				for (const element of bindings.elements) {
					const importedName = element.propertyName
						? element.propertyName.text
						: element.name.text;
					const localName = element.name.text;
					addNamedImport(moduleName, importedName, localName);
				}
			}
		}
	}

	function handleVariableStatement(node: ts.VariableStatement): void {
		for (const declaration of node.declarationList.declarations) {
			handleVariableDeclaration(declaration);
		}
	}

	function handleVariableDeclaration(
		declaration: ts.VariableDeclaration,
	): void {
		if (!declaration.initializer) {
			return;
		}

		// Check if initializer is require() call
		if (
			ts.isCallExpression(declaration.initializer) &&
			ts.isIdentifier(declaration.initializer.expression) &&
			declaration.initializer.expression.text === "require"
		) {
			const args = declaration.initializer.arguments;
			if (args.length !== 1 || !ts.isStringLiteral(args[0])) {
				return;
			}

			const moduleName = args[0].text;
			const name = declaration.name;

			// Direct assignment: const cp = require('child_process')
			if (ts.isIdentifier(name)) {
				addModuleAlias(moduleName, name.text);
			}

			// Destructuring: const { exec, spawn: sp } = require('child_process')
			// Also handles: const { "exec": execute } = require('child_process')
			if (ts.isObjectBindingPattern(name)) {
				for (const element of name.elements) {
					// Extract property name (what's imported from module)
					let importedName: string | null = null;

					if (element.propertyName) {
						// Property name exists: { "exec": execute } or { [key]: execute }
						importedName = getBindingPropertyName(element.propertyName);
						// If dynamic (returns null), skip this binding entirely
						if (importedName === null) {
							continue;
						}
					} else {
						// No property name: { exec } - name is both imported and local
						importedName = ts.isIdentifier(element.name)
							? element.name.text
							: null;
					}

					// Extract local name (what it's called in this file)
					// NOTE: Nested binding patterns on the local side are ignored by design
					const localName = ts.isIdentifier(element.name)
						? element.name.text
						: null;

					if (importedName && localName) {
						addNamedImport(moduleName, importedName, localName);
					}
				}
			}
		}
	}

	/**
	 * Extract property name from binding element property.
	 * Supports: identifiers, string literals, numeric literals, and static computed properties.
	 * Returns null for truly dynamic computed keys.
	 */
	function getBindingPropertyName(prop?: ts.PropertyName): string | null {
		if (!prop) {
			return null;
		}

		// Handle: { exec } or { exec: execute }
		if (ts.isIdentifier(prop)) {
			return prop.text;
		}

		// Handle: { "exec": execute } or { 'exec': execute }
		if (ts.isStringLiteral(prop)) {
			return prop.text;
		}

		// Handle: { 123: execute }
		if (ts.isNumericLiteral(prop)) {
			return prop.text;
		}

		// Handle: { ["exec"]: execute } with static string/number
		if (ts.isComputedPropertyName(prop)) {
			const expr = prop.expression;

			if (ts.isStringLiteral(expr) || ts.isNumericLiteral(expr)) {
				return expr.text;
			}

			// Handle template literals without substitutions: { [`exec`]: execute }
			if (ts.isNoSubstitutionTemplateLiteral(expr)) {
				return expr.text;
			}

			// Ignore truly dynamic computed keys: { [variable]: execute }
			return null;
		}

		return null;
	}

	function handleDynamicImport(node: ts.CallExpression): void {
		// Dynamic import: import('crypto')
		if (node.expression.kind === ts.SyntaxKind.ImportKeyword) {
			const args = node.arguments;
			if (args.length === 1 && ts.isStringLiteral(args[0])) {
				const moduleName = args[0].text;
				// Track dynamic imports as module aliases (no specific local name)
				addModuleAlias(moduleName, "__dynamic__");
			}
		}
	}

	function addModuleAlias(moduleName: string, localName: string): void {
		if (!moduleAliases.has(moduleName)) {
			moduleAliases.set(moduleName, new Set());
		}
		moduleAliases.get(moduleName)?.add(localName);
	}

	function addNamedImport(
		moduleName: string,
		importedName: string,
		localName: string,
	): void {
		if (!namedImports.has(moduleName)) {
			namedImports.set(moduleName, new Map());
		}
		namedImports.get(moduleName)?.set(importedName, localName);
	}

	visit(sourceFile);

	return {
		moduleAliases,
		namespaceAliases,
		namedImports,
	};
}

/**
 * Check if an identifier refers to a specific module based on alias maps.
 *
 * Searches across all three alias types:
 * - Module aliases (default imports and requires)
 * - Namespace aliases (import * as)
 * - Named imports (destructured and renamed imports)
 *
 * @param identifier - Local identifier name to check
 * @param moduleName - Module name to check against (e.g., 'child_process')
 * @param aliasMaps - Alias maps built from source file
 * @returns True if identifier refers to the specified module
 *
 * @example
 * ```ts
 * // Given: import cp from 'child_process'
 * isAliasOf('cp', 'child_process', maps) // true
 * isAliasOf('fs', 'child_process', maps) // false
 * ```
 */
export function isAliasOf(
	identifier: string,
	moduleName: string,
	aliasMaps: AliasMaps,
): boolean {
	// Check module aliases (default imports and requires)
	const moduleAliases = aliasMaps.moduleAliases.get(moduleName);
	if (moduleAliases?.has(identifier)) {
		return true;
	}

	// Check namespace aliases (import * as)
	const namespaceAlias = aliasMaps.namespaceAliases.get(moduleName);
	if (namespaceAlias === identifier) {
		return true;
	}

	// Check named imports
	const namedImportMap = aliasMaps.namedImports.get(moduleName);
	if (namedImportMap) {
		for (const localName of namedImportMap.values()) {
			if (localName === identifier) {
				return true;
			}
		}
	}

	return false;
}
