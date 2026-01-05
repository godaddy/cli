import * as fs from "node:fs";
import { join } from "node:path";
import type { Result } from "../../shared/types";

/**
 * The type of module system used by the entry point
 */
export type SourceType = "module" | "commonjs";

/**
 * How the entry point was resolved
 */
export type ResolvedFrom =
	| "exports.import"
	| "module"
	| "main"
	| "exports"
	| "fallback";

/**
 * Successful entry point resolution result
 */
export interface EntryPointResolution {
	/** Absolute path to the entry point file */
	entryPath: string;
	/** Type of module system (module for ESM, commonjs for CJS) */
	sourceType: SourceType;
	/** Which package.json field or fallback was used to resolve */
	resolvedFrom: ResolvedFrom;
}

/**
 * Options for entry point resolution
 */
export interface ResolveEntryPointOptions {
	/** Absolute path to the extension package directory */
	packageDir: string;
	/** Parsed package.json object */
	packageJson: Record<string, unknown>;
}

/**
 * Resolves the entry point file for an extension package.
 *
 * This pure function determines which source file should be used as the entry point
 * for bundling an extension. It follows a priority order based on package.json fields
 * and conventional file locations.
 *
 * Resolution priority:
 * 1. package.json `exports["."].import` - Modern ESM exports
 * 2. package.json `module` - ESM module entry
 * 3. package.json `main` - Traditional entry (CJS or ESM)
 * 4. package.json `exports` (string) - Simple exports field
 * 5. Fallback paths in order:
 *    - src/index.ts
 *    - src/index.tsx
 *    - src/index.mts
 *    - index.ts
 *    - index.mts
 *    - src/index.js (warns)
 *    - index.js (warns)
 *
 * @param options - Configuration containing package directory and parsed package.json
 * @returns Result containing the entry point resolution or an error with actionable message
 *
 * @example
 * ```typescript
 * const result = resolveEntryPoint({
 *   packageDir: "/path/to/extension",
 *   packageJson: { name: "my-extension", module: "./src/index.ts" }
 * });
 *
 * if (result.success) {
 *   console.log(result.data.entryPath); // "/path/to/extension/src/index.ts"
 *   console.log(result.data.resolvedFrom); // "module"
 * } else {
 *   console.error(result.error.message);
 * }
 * ```
 */
export function resolveEntryPoint(
	options: ResolveEntryPointOptions,
): Result<EntryPointResolution> {
	const { packageDir, packageJson } = options;
	const sourceType = getSourceType(packageJson);

	// Priority 1: exports["."].import
	const exports = packageJson.exports;
	if (exports && typeof exports === "object" && !Array.isArray(exports)) {
		const dotExport = (exports as Record<string, unknown>)["."];
		if (
			dotExport &&
			typeof dotExport === "object" &&
			!Array.isArray(dotExport)
		) {
			const importPath = (dotExport as Record<string, unknown>)
				.import as string;
			if (importPath) {
				const resolved = tryResolvePath(packageDir, importPath);
				if (resolved) {
					return {
						success: true,
						data: {
							entryPath: resolved,
							sourceType: "module",
							resolvedFrom: "exports.import",
						},
					};
				}
			}
		}
	}

	// Priority 2: module field
	const moduleField = packageJson.module as string | undefined;
	if (moduleField) {
		const resolved = tryResolvePath(packageDir, moduleField);
		if (resolved) {
			return {
				success: true,
				data: {
					entryPath: resolved,
					sourceType: "module",
					resolvedFrom: "module",
				},
			};
		}
	}

	// Priority 3: main field
	const mainField = packageJson.main as string | undefined;
	if (mainField) {
		const resolved = tryResolvePath(packageDir, mainField);
		if (resolved) {
			return {
				success: true,
				data: {
					entryPath: resolved,
					sourceType,
					resolvedFrom: "main",
				},
			};
		}
	}

	// Priority 4: exports (string)
	if (typeof exports === "string") {
		const resolved = tryResolvePath(packageDir, exports);
		if (resolved) {
			return {
				success: true,
				data: {
					entryPath: resolved,
					sourceType,
					resolvedFrom: "exports",
				},
			};
		}
	}

	// Priority 5: Fallback paths
	const fallbackPaths = [
		"src/index.ts",
		"src/index.tsx",
		"src/index.mts",
		"index.ts",
		"index.mts",
		"src/index.js",
		"index.js",
	];

	for (const fallbackPath of fallbackPaths) {
		const resolved = tryResolvePath(packageDir, fallbackPath);
		if (resolved) {
			return {
				success: true,
				data: {
					entryPath: resolved,
					sourceType,
					resolvedFrom: "fallback",
				},
			};
		}
	}

	// No entry point found
	return {
		success: false,
		error: new Error(
			`No entry point found for package in ${packageDir}. Add one of the following to package.json: "exports['.'].import", "module", or "main". Or create one of these files: ${fallbackPaths.join(", ")}`,
		),
	};
}

/**
 * Attempts to resolve and validate a path relative to the package directory.
 *
 * @param packageDir - Absolute path to the package directory
 * @param relativePath - Relative path from package.json field or fallback
 * @returns Absolute path if file exists, null otherwise
 *
 * @example
 * ```typescript
 * const path = tryResolvePath("/path/to/pkg", "./src/index.ts");
 * // Returns: "/path/to/pkg/src/index.ts" if exists, null otherwise
 * ```
 */
export function tryResolvePath(
	packageDir: string,
	relativePath: string,
): string | null {
	// Normalize the relative path (remove leading ./)
	const normalized = relativePath.startsWith("./")
		? relativePath.slice(2)
		: relativePath;

	const absolutePath = join(packageDir, normalized);

	// Check if file exists
	if (fs.existsSync(absolutePath)) {
		return absolutePath;
	}

	return null;
}

/**
 * Determines the source type (module vs commonjs) from package.json.
 *
 * Checks the package.json "type" field:
 * - "module" → module (ESM)
 * - undefined or "commonjs" → commonjs (CJS)
 *
 * @param packageJson - Parsed package.json object
 * @returns The determined source type
 *
 * @example
 * ```typescript
 * const type = getSourceType({ type: "module" });
 * // Returns: "module"
 * ```
 */
export function getSourceType(
	packageJson: Record<string, unknown>,
): SourceType {
	const type = packageJson.type as string | undefined;
	return type === "module" ? "module" : "commonjs";
}
