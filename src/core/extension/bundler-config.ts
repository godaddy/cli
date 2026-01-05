/**
 * Pure esbuild configuration generation for extension bundling.
 * Provides standard build options for compiling extensions to ESM artifacts.
 */

import type { BuildOptions } from "esbuild";

export type ExtensionType = "embed" | "checkout" | "blocks";

/**
 * Options for building esbuild configuration.
 */
export interface EsbuildConfigOptions {
	entryPath: string;
	tsconfigPath?: string;
	extensionType?: ExtensionType;
	extensionDir?: string;
	overrides?: Partial<BuildOptions>;
}

/**
 * Gets the external dependencies based on extension type.
 * - blocks: React is provided by the platform, so externalize it
 * - embed/checkout: Rendered in iframes, must bundle React (aliased to Preact)
 */
function getExternals(extensionType?: ExtensionType): string[] {
	const baseExternals = ["node:*", "@wsblocks/*"];

	if (extensionType === "blocks") {
		return [...baseExternals, "react", "react/*", "react-dom", "react-dom/*"];
	}

	return baseExternals;
}

/**
 * Gets React-to-Preact aliases for embed/checkout extensions.
 * Preact/compat provides React API compatibility at ~3KB vs ~40KB.
 * Blocks extensions use platform-provided React, so no aliasing needed.
 */
function getAliases(
	extensionType?: ExtensionType,
): Record<string, string> | undefined {
	if (extensionType === "blocks") {
		return undefined;
	}

	return {
		react: "preact/compat",
		"react-dom": "preact/compat",
		"react/jsx-runtime": "preact/jsx-runtime",
	};
}

/**
 * Gets platform and format based on extension type.
 * - blocks: Node platform, ESM format (runs server-side)
 * - embed/checkout: Browser platform, IIFE format (runs in iframe)
 */
function getPlatformConfig(extensionType?: ExtensionType): {
	platform: "node" | "browser";
	format: "esm" | "iife";
	target: string;
} {
	if (extensionType === "blocks") {
		return { platform: "node", format: "esm", target: "node22" };
	}

	return { platform: "browser", format: "iife", target: "es2020" };
}

/**
 * Builds esbuild configuration options for bundling an extension.
 * Returns a pure configuration object that can be passed to esbuild.build().
 *
 * Standard configuration:
 * - Format: ESM
 * - Platform: Node.js
 * - Target: Node 22
 * - Minification: Enabled
 * - Sourcemaps: External (.map file)
 * - Write: False (capture in memory for hashing)
 * - External: Node built-ins, @wsblocks/* (React externalized only for blocks)
 *
 * @param options - Configuration options including entry path and extension type
 * @returns Complete esbuild BuildOptions configuration
 *
 * @example
 * ```ts
 * const config = buildEsbuildOptions({
 *   entryPath: "/path/to/extension/src/index.ts",
 *   tsconfigPath: "/path/to/tsconfig.json",
 *   extensionType: "blocks",
 * });
 * const result = await esbuild.build(config);
 * ```
 */
export function buildEsbuildOptions(
	options: EsbuildConfigOptions,
): BuildOptions;
/**
 * @deprecated Use the options object form instead
 */
export function buildEsbuildOptions(
	entryPath: string,
	tsconfigPath?: string,
	overrides?: Partial<BuildOptions>,
): BuildOptions;
export function buildEsbuildOptions(
	entryPathOrOptions: string | EsbuildConfigOptions,
	tsconfigPath?: string,
	overrides?: Partial<BuildOptions>,
): BuildOptions {
	// Handle both signatures
	const options: EsbuildConfigOptions =
		typeof entryPathOrOptions === "string"
			? { entryPath: entryPathOrOptions, tsconfigPath, overrides }
			: entryPathOrOptions;

	const { platform, format, target } = getPlatformConfig(options.extensionType);
	const alias = getAliases(options.extensionType);

	// Build nodePaths for module resolution - include extension's node_modules
	const nodePaths: string[] = [];
	if (options.extensionDir) {
		nodePaths.push(`${options.extensionDir}/node_modules`);
	}

	const baseConfig: BuildOptions = {
		entryPoints: [options.entryPath],
		bundle: true,
		format,
		platform,
		target,
		minify: true,
		sourcemap: "external",
		write: false,
		logLevel: "silent",
		external: getExternals(options.extensionType),
		outExtension: { ".js": ".mjs" },
		outdir: "out", // Required for external sourcemaps with write: false
		...(options.tsconfigPath && { tsconfig: options.tsconfigPath }),
		...(alias && { alias }),
		...(nodePaths.length > 0 && { nodePaths }),
	};

	return {
		...baseConfig,
		...options.overrides,
	};
}
