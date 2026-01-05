/**
 * Extension bundler service.
 * Orchestrates esbuild bundling with temp directory management and error handling.
 */

import { existsSync } from "node:fs";
import { mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import {
	type ExtensionType,
	buildEsbuildOptions,
} from "@core/extension/bundler-config";
import {
	buildArtifactName,
	computeHash,
	formatTimestamp,
	shortHash,
} from "@core/extension/naming";
import type { Result } from "@shared/types";
import * as esbuild from "esbuild";
import { getLogger } from "../logger";

/**
 * Result of a successful bundle operation.
 */
export interface BundleResult {
	packageName: string;
	version?: string;
	artifactPath: string;
	artifactName: string;
	size: number;
	sha256: string;
	sourcemapPath?: string;
}

/**
 * Options for bundling an extension.
 */
export interface BundleOptions {
	repoRoot: string;
	timestamp?: string;
	extensionDir?: string;
	extensionType?: ExtensionType;
}

/**
 * Package metadata for bundling.
 */
export interface ExtensionPackage {
	name: string;
	version?: string;
}

/**
 * Resolves TypeScript configuration file path.
 * Checks extension directory first, then repo root, then returns undefined.
 *
 * @param extensionDir - Directory containing the extension
 * @param repoRoot - Repository root directory
 * @returns Path to tsconfig.json or undefined
 */
export function resolveTsConfig(
	extensionDir: string,
	repoRoot: string,
): string | undefined {
	const localTsConfig = join(extensionDir, "tsconfig.json");
	if (existsSync(localTsConfig)) {
		return localTsConfig;
	}

	const rootTsConfig = join(repoRoot, "tsconfig.json");
	if (existsSync(rootTsConfig)) {
		return rootTsConfig;
	}

	return undefined;
}

/**
 * Creates temporary directory for bundling artifacts.
 * Structure: ${os.tmpdir()}/gd-cli/${repoName}/deploy-${timestamp}/
 *
 * @param repoRoot - Repository root path
 * @param timestamp - Timestamp for deploy session
 * @returns Path to temp directory
 */
export function createTempDirectory(
	repoRoot: string,
	timestamp: string,
): string {
	const repoName = basename(repoRoot);
	return join(tmpdir(), "gd-cli", repoName, `deploy-${timestamp}`);
}

/**
 * Cleans up temporary directory and all contents.
 *
 * @param tempDir - Directory to remove
 */
export async function cleanupTempDirectory(tempDir: string): Promise<void> {
	if (existsSync(tempDir)) {
		await rm(tempDir, { recursive: true, force: true });
	}
}

/**
 * Bundles an extension from its directory (convenience wrapper).
 * Resolves entry point and delegates to bundleExtension.
 *
 * This is the recommended entry point for bundling - it matches the
 * scanExtension pattern used in security scanning.
 *
 * @param extensionDir - Absolute path to extension directory
 * @param options - Bundle options
 * @returns Result with BundleResult or error
 *
 * @example
 * ```ts
 * // Loop over extensions (same pattern as security scanning)
 * for (const extension of extensions) {
 *   const result = await bundleExtensionFromDir(extension.dir, { repoRoot });
 *   if (!result.success) {
 *     console.error(`Failed to bundle ${extension.name}: ${result.error}`);
 *   }
 * }
 * ```
 */
export async function bundleExtensionFromDir(
	extensionDir: string,
	options: BundleOptions,
): Promise<Result<BundleResult>> {
	const { readPackageJson } = await import("./workspace");
	const { resolveEntryPoint } = await import("../../core/extension/entry");

	// Read package.json
	const packageJsonPath = join(extensionDir, "package.json");
	const pkgResult = readPackageJson(packageJsonPath);
	if (!pkgResult.success || !pkgResult.data) {
		return {
			success: false,
			error: new Error(
				`Failed to read package.json: ${pkgResult.error?.message || "Unknown error"}`,
			),
		};
	}

	const packageJson = pkgResult.data;
	const name = packageJson.name as string;
	const version = packageJson.version as string | undefined;

	// Resolve entry point
	const entryResult = resolveEntryPoint({
		packageDir: extensionDir,
		packageJson,
	});

	if (!entryResult.success || !entryResult.data) {
		return {
			success: false,
			error: new Error(
				`Failed to resolve entry point: ${entryResult.error?.message || "Unknown error"}`,
			),
		};
	}

	const { entryPath } = entryResult.data;

	// Bundle the extension
	return bundleExtension({ name, version }, entryPath, {
		...options,
		extensionDir,
	});
}

/**
 * Bundles an extension into an ESM artifact with sourcemap.
 *
 * Process:
 * 1. Create temp directory per extension
 * 2. Resolve tsconfig (local → root → undefined)
 * 3. Run esbuild with write: false
 * 4. Extract .mjs and .mjs.map from outputFiles
 * 5. Compute hash of bundle content
 * 6. Build artifact filename with sanitization
 * 7. Update sourceMappingURL to match renamed map file
 * 8. Write both files to temp directory
 * 9. Return BundleResult with metadata
 *
 * @param pkg - Package metadata (name, version)
 * @param entryPath - Absolute path to entry point
 * @param options - Bundle options
 * @returns Result with BundleResult or error
 */
export async function bundleExtension(
	pkg: ExtensionPackage,
	entryPath: string,
	options: BundleOptions,
): Promise<Result<BundleResult>> {
	const logger = getLogger();
	const startTime = Date.now();
	const timestamp = options.timestamp || formatTimestamp();

	// Create temp directory structure
	const tempRoot = createTempDirectory(options.repoRoot, timestamp);
	const extensionTempDir = join(tempRoot, pkg.name);

	try {
		await mkdir(extensionTempDir, { recursive: true });

		// Resolve tsconfig
		const extensionDir = options.extensionDir ?? join(entryPath, "..");
		const tsconfigPath = resolveTsConfig(extensionDir, options.repoRoot);

		// Build esbuild config
		const config = buildEsbuildOptions({
			entryPath,
			tsconfigPath,
			extensionType: options.extensionType,
			extensionDir,
		});

		// Run esbuild
		let buildResult: esbuild.BuildResult;
		try {
			buildResult = await esbuild.build(config);
		} catch (error) {
			return {
				success: false,
				error: new Error(
					`ESBUILD_ERROR: ${error instanceof Error ? error.message : String(error)}`,
				),
			};
		}

		// Extract output files
		const outputFiles = buildResult.outputFiles;
		if (!outputFiles || outputFiles.length === 0) {
			return {
				success: false,
				error: new Error("No output files generated by esbuild"),
			};
		}

		const mjsFile = outputFiles.find((f) => f.path.endsWith(".mjs"));
		const mapFile = outputFiles.find((f) => f.path.endsWith(".mjs.map"));

		if (!mjsFile) {
			return {
				success: false,
				error: new Error("No .mjs file generated by esbuild"),
			};
		}

		// Prepare bundle content (will be modified for sourcemap URL)
		let bundleContent = mjsFile.text;

		// First pass: strip any existing sourceMappingURL for consistent hashing
		// Also strip trailing newlines for consistency
		const bundleWithoutSourceMap = bundleContent
			.replace(/^\/\/# sourceMappingURL=.*$/m, "")
			.trimEnd();

		// Compute hash on content WITHOUT sourceMappingURL to avoid circularity
		// (since the artifact name contains the hash and is embedded in the URL)
		const sha256 = computeHash(Buffer.from(bundleWithoutSourceMap));
		const hash = shortHash(sha256);

		// Build artifact name using the hash
		const artifactName = buildArtifactName(
			pkg.name,
			pkg.version || "0.0.0",
			timestamp,
			hash,
		);

		// Update sourceMappingURL in bundle to match renamed map file
		// Use the version without existing sourcemap, then add the new one
		bundleContent = bundleWithoutSourceMap;
		if (mapFile) {
			const mapName = `${artifactName}.map`;
			// Always append the sourceMappingURL at the end
			bundleContent += `\n//# sourceMappingURL=${mapName}\n`;
		}

		// Write bundle file
		const artifactPath = join(extensionTempDir, artifactName);
		await writeFile(artifactPath, bundleContent, "utf-8");

		// Write sourcemap file if present
		let sourcemapPath: string | undefined;
		if (mapFile) {
			sourcemapPath = `${artifactPath}.map`;
			await writeFile(sourcemapPath, mapFile.text, "utf-8");
		}

		const durationMs = Date.now() - startTime;
		const size = Buffer.byteLength(bundleContent);

		// Emit debug log
		logger.debug({
			type: "bundle",
			extension: pkg.name,
			entry: entryPath,
			size,
			sha256,
			durationMs,
		});

		return {
			success: true,
			data: {
				packageName: pkg.name,
				version: pkg.version,
				artifactPath,
				artifactName,
				size,
				sha256,
				sourcemapPath,
			},
		};
	} catch (error) {
		return {
			success: false,
			error: error instanceof Error ? error : new Error(String(error)),
		};
	}
	// Note: Cleanup is not performed here to keep artifactPath valid for downstream phases.
	// The orchestrator/deploy command should call cleanupTempDirectory after artifacts are consumed.
}
