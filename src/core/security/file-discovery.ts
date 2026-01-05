import type { Stats } from "node:fs";
import { readdir, stat } from "node:fs/promises";
import { join, resolve } from "node:path";
import type { Result } from "../../shared/types";
import { getSecurityConfig, shouldExcludeFile } from "./config";

/**
 * Supported source file extensions for security scanning
 */
const SOURCE_EXTENSIONS = [".js", ".ts", ".jsx", ".tsx", ".mjs", ".cjs"];

/**
 * Recursively discover all source files to scan in an extension directory.
 * Applies security configuration exclusion patterns (node_modules, dist, build, __tests__).
 *
 * @param rootPath - Absolute or relative path to extension directory
 * @returns Result containing array of absolute file paths to scan, or error
 *
 * @example
 * ```ts
 * const result = await findFilesToScan('/path/to/extension');
 * if (result.success && result.data) {
 *   console.log(`Found ${result.data.length} files to scan`);
 *   for (const file of result.data) {
 *     console.log(file);
 *   }
 * }
 * ```
 */
export async function findFilesToScan(
	rootPath: string,
): Promise<Result<string[]>> {
	try {
		const config = getSecurityConfig();
		const absoluteRoot = resolve(rootPath);

		// Verify root directory exists
		try {
			const rootStats = await stat(absoluteRoot);
			if (!rootStats.isDirectory()) {
				return {
					success: false,
					error: new Error(`Path is not a directory: ${absoluteRoot}`),
				};
			}
		} catch (error) {
			return {
				success: false,
				error: error instanceof Error ? error : new Error(String(error)),
			};
		}

		const files: string[] = [];
		await traverseDirectory(absoluteRoot, files, config);

		return {
			success: true,
			data: files,
		};
	} catch (error) {
		return {
			success: false,
			error: error instanceof Error ? error : new Error(String(error)),
		};
	}
}

/**
 * Recursively traverse a directory and collect source files.
 * Skips directories and files matching exclusion patterns.
 *
 * @param dirPath - Absolute path to directory
 * @param files - Array to accumulate file paths (mutated)
 * @param config - Security configuration with exclusion patterns
 */
async function traverseDirectory(
	dirPath: string,
	files: string[],
	config: ReturnType<typeof getSecurityConfig>,
): Promise<void> {
	// Check if directory should be excluded
	if (shouldExcludeFile(dirPath, config)) {
		return;
	}

	let entries: string[];
	try {
		entries = await readdir(dirPath);
	} catch (error) {
		// Skip directories we can't read (permission issues, etc.)
		return;
	}

	for (const entry of entries) {
		const fullPath = join(dirPath, entry);

		// Check exclusions before stat
		if (shouldExcludeFile(fullPath, config)) {
			continue;
		}

		let stats: Stats;
		try {
			stats = await stat(fullPath);
		} catch (error) {
			// Skip files/dirs we can't stat
			continue;
		}

		if (stats.isDirectory()) {
			// Recurse into subdirectory
			await traverseDirectory(fullPath, files, config);
		} else if (stats.isFile() && isSourceFile(fullPath)) {
			// Add source file to list
			files.push(fullPath);
		}
	}
}

/**
 * Check if a file path has a supported source file extension.
 *
 * @param filePath - Path to file
 * @returns True if file has .js, .ts, .jsx, .tsx, .mjs, or .cjs extension
 */
function isSourceFile(filePath: string): boolean {
	return SOURCE_EXTENSIONS.some((ext) => filePath.endsWith(ext));
}
