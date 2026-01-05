import * as fs from "node:fs";
import { join } from "node:path";
import type { Result } from "../../shared/types";

/**
 * Package manager types supported by the CLI
 */
export type PackageManager = "pnpm" | "yarn" | "npm" | "unknown";

/**
 * Represents a detected extension package in the workspace
 */
export interface ExtensionPackage {
	/** Package name from package.json */
	name: string;
	/** Package version from package.json */
	version?: string;
	/** Absolute path to the extension directory */
	dir: string;
	/** Absolute path to the package.json file */
	packageJsonPath: string;
	/** Detected package manager for the workspace */
	packageManager: PackageManager;
}

/**
 * Options for detecting extensions in the workspace
 */
export interface DetectExtensionsOptions {
	/** Root directory of the repository (defaults to process.cwd()) */
	repoRoot?: string;
	/** Name of the extensions directory (defaults to "extensions") */
	extensionsDir?: string;
}

/**
 * Gets all extension packages in the workspace by scanning the extensions directory.
 *
 * This function scans the configured extensions directory (default: ./extensions) and
 * identifies all subdirectories containing a valid package.json file as extension packages.
 *
 * @param options - Configuration options for extension detection
 * @returns Promise resolving to a result containing an array of detected extension packages or an error
 *
 * @example
 * ```typescript
 * // Get extensions in default ./extensions directory
 * const result = await getExtensions();
 * if (result.success) {
 *   console.log(result.data); // ExtensionPackage[]
 * }
 *
 * // Get extensions in custom directory
 * const result = await getExtensions({
 *   repoRoot: "/path/to/repo",
 *   extensionsDir: "my-extensions"
 * });
 * ```
 */
export async function getExtensions(
	options?: DetectExtensionsOptions,
): Promise<Result<ExtensionPackage[]>> {
	const repoRoot = options?.repoRoot ?? process.cwd();
	const extensionsDir = options?.extensionsDir ?? "extensions";
	const extensionsPath = join(repoRoot, extensionsDir);

	// Detect package manager once for all extensions
	const packageManager = detectPackageManager(repoRoot);

	// Try extensions directory first
	if (fs.existsSync(extensionsPath)) {
		// Check if it's a directory
		const stats = fs.statSync(extensionsPath);
		if (!stats.isDirectory()) {
			return {
				success: false,
				error: new Error(
					`Extensions path ${extensionsPath} exists but is not a directory`,
				),
			};
		}

		// Read all subdirectories in the extensions directory
		const entries = fs.readdirSync(extensionsPath, { withFileTypes: true });
		const extensions: ExtensionPackage[] = [];

		for (const entry of entries) {
			// Skip files, only process directories
			if (!entry.isDirectory()) {
				continue;
			}

			const extensionDir = join(extensionsPath, entry.name);
			const packageJsonPath = join(extensionDir, "package.json");

			// Check if package.json exists
			if (!fs.existsSync(packageJsonPath)) {
				continue;
			}

			// Read and parse package.json
			const packageJsonResult = readPackageJson(packageJsonPath);
			if (!packageJsonResult.success) {
				return {
					success: false,
					error: new Error(
						`Failed to read package.json for extension "${entry.name}": ${packageJsonResult.error?.message}`,
					),
				};
			}

			const packageJson = packageJsonResult.data;
			const name = packageJson?.name as string | undefined;
			const version = packageJson?.version as string | undefined;

			if (!name) {
				return {
					success: false,
					error: new Error(
						`Extension in directory "${entry.name}" has invalid package.json: missing "name" field`,
					),
				};
			}

			extensions.push({
				name,
				version,
				dir: extensionDir,
				packageJsonPath,
				packageManager,
			});
		}

		return { success: true, data: extensions };
	}

	// Fallback to workspaces
	const rootPackageJsonPath = join(repoRoot, "package.json");
	if (!fs.existsSync(rootPackageJsonPath)) {
		return {
			success: false,
			error: new Error(
				`No extensions directory found at ${extensionsPath} and no package.json found at repository root`,
			),
		};
	}

	const rootPackageResult = readPackageJson(rootPackageJsonPath);
	if (!rootPackageResult.success) {
		return {
			success: false,
			error: new Error(
				`Failed to read root package.json: ${rootPackageResult.error?.message}`,
			),
		};
	}

	const rootPackageJson = rootPackageResult.data;
	const workspaces = rootPackageJson?.workspaces as string[] | undefined;

	if (!workspaces || !Array.isArray(workspaces) || workspaces.length === 0) {
		return {
			success: false,
			error: new Error(
				`No extensions directory found at ${extensionsPath} and no workspaces defined in package.json. Either create ${extensionsDir}/ directory or add workspaces to package.json.`,
			),
		};
	}

	// Process workspaces
	const extensions: ExtensionPackage[] = [];

	for (const workspace of workspaces) {
		// Resolve glob patterns (simple support for */pattern)
		const workspacePaths: string[] = [];

		if (workspace.includes("*")) {
			// Simple glob support: packages/* or apps/*
			const baseDir = workspace.replace("/*", "");
			const basePath = join(repoRoot, baseDir);

			if (fs.existsSync(basePath) && fs.statSync(basePath).isDirectory()) {
				const entries = fs.readdirSync(basePath, { withFileTypes: true });
				for (const entry of entries) {
					if (entry.isDirectory()) {
						workspacePaths.push(join(basePath, entry.name));
					}
				}
			}
		} else {
			// Direct path
			workspacePaths.push(join(repoRoot, workspace));
		}

		// Check each workspace path
		for (const workspacePath of workspacePaths) {
			const packageJsonPath = join(workspacePath, "package.json");

			if (!fs.existsSync(packageJsonPath)) {
				continue;
			}

			const packageJsonResult = readPackageJson(packageJsonPath);
			if (!packageJsonResult.success) {
				continue; // Skip invalid package.json in workspaces
			}

			const packageJson = packageJsonResult.data;

			// Check if this workspace is marked as an extension
			const godaddy = packageJson?.godaddy as
				| Record<string, unknown>
				| boolean
				| undefined;
			const isExtension =
				(typeof godaddy === "object" &&
					!Array.isArray(godaddy) &&
					(godaddy.extension === true || godaddy.type === "extension")) ||
				godaddy === true;

			if (!isExtension) {
				continue;
			}

			const name = packageJson?.name as string | undefined;
			const version = packageJson?.version as string | undefined;

			if (!name) {
				continue; // Skip packages without name
			}

			extensions.push({
				name,
				version,
				dir: workspacePath,
				packageJsonPath,
				packageManager,
			});
		}
	}

	return { success: true, data: extensions };
}

/**
 * Detects the package manager being used in the workspace.
 *
 * Detection priority:
 * 1. package.json "packageManager" field → parse manager type
 * 2. pnpm-lock.yaml → pnpm
 * 3. yarn.lock → yarn
 * 4. package-lock.json → npm
 * 5. none found → unknown
 *
 * @param repoRoot - Root directory of the repository to check
 * @returns The detected package manager type
 *
 * @example
 * ```typescript
 * const pm = detectPackageManager("/path/to/repo");
 * // Returns: "pnpm" | "yarn" | "npm" | "unknown"
 * ```
 */
export function detectPackageManager(repoRoot: string): PackageManager {
	// First, check package.json's packageManager field
	const rootPackageJsonPath = join(repoRoot, "package.json");
	if (fs.existsSync(rootPackageJsonPath)) {
		const result = readPackageJson(rootPackageJsonPath);
		if (result.success && result.data) {
			const packageManager = result.data.packageManager as string | undefined;
			if (packageManager) {
				// packageManager format is like "pnpm@8.0.0" or "yarn@3.0.0"
				if (packageManager.startsWith("pnpm")) return "pnpm";
				if (packageManager.startsWith("yarn")) return "yarn";
				if (packageManager.startsWith("npm")) return "npm";
			}
		}
	}

	// Fallback to lockfile detection
	if (fs.existsSync(join(repoRoot, "pnpm-lock.yaml"))) return "pnpm";
	if (fs.existsSync(join(repoRoot, "yarn.lock"))) return "yarn";
	if (fs.existsSync(join(repoRoot, "package-lock.json"))) return "npm";

	return "unknown";
}

/**
 * Reads and parses a package.json file from the given directory.
 *
 * @param packageJsonPath - Absolute path to the package.json file
 * @returns Result containing parsed package.json object or an error if file doesn't exist or is invalid
 *
 * @example
 * ```typescript
 * const result = readPackageJson("/path/to/extension/package.json");
 * if (result.success) {
 *   console.log(result.data.name, result.data.version);
 * }
 * ```
 */
export function readPackageJson(
	packageJsonPath: string,
): Result<Record<string, unknown>> {
	try {
		// Check if file exists
		if (!fs.existsSync(packageJsonPath)) {
			return {
				success: false,
				error: new Error(`package.json not found at ${packageJsonPath}`),
			};
		}

		// Read file contents
		const content = fs.readFileSync(packageJsonPath, "utf-8");

		// Parse JSON
		const parsed = JSON.parse(content) as Record<string, unknown>;

		return { success: true, data: parsed };
	} catch (error) {
		return {
			success: false,
			error: new Error(
				`Failed to parse package.json at ${packageJsonPath}: ${error instanceof Error ? error.message : String(error)}`,
			),
		};
	}
}
