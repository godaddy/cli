/**
 * Extension bundler service.
 * Orchestrates esbuild bundling with temp directory management and error handling.
 */

import * as nodeFs from "node:fs";
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
import { FileSystem } from "@effect/platform/FileSystem";
import * as Effect from "effect/Effect";
import * as esbuild from "esbuild";
import { ConfigurationError } from "../../effect/errors";
import { fileExists } from "../../effect/fs-utils";
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
 * Uses node:fs directly since this is a simple sync check.
 */
export function resolveTsConfig(
  extensionDir: string,
  repoRoot: string,
): string | undefined {
  const localTsConfig = join(extensionDir, "tsconfig.json");
  if (nodeFs.existsSync(localTsConfig)) {
    return localTsConfig;
  }

  const rootTsConfig = join(repoRoot, "tsconfig.json");
  if (nodeFs.existsSync(rootTsConfig)) {
    return rootTsConfig;
  }

  return undefined;
}

/**
 * Creates temporary directory for bundling artifacts.
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
 */
export function cleanupTempDirectoryEffect(
  tempDir: string,
): Effect.Effect<void, ConfigurationError, FileSystem> {
  return Effect.gen(function* () {
    const fs = yield* FileSystem;
    const exists = yield* fileExists(tempDir);
    if (exists) {
      yield* fs.remove(tempDir, { recursive: true }).pipe(
        Effect.mapError(
          (error) =>
            new ConfigurationError({
              message: `Failed to cleanup temp directory: ${error.message}`,
              userMessage: "Failed to cleanup temporary build files",
            }),
        ),
      );
    }
  });
}

/**
 * Bundles an extension from its directory (convenience wrapper).
 */
export function bundleExtensionFromDirEffect(
  extensionDir: string,
  options: BundleOptions,
): Effect.Effect<BundleResult, ConfigurationError, FileSystem> {
  return Effect.gen(function* () {
    const fs = yield* FileSystem;

    // Read package.json
    const packageJsonPath = join(extensionDir, "package.json");
    const packageJsonContent = yield* fs.readFileString(packageJsonPath).pipe(
      Effect.mapError(
        (error) =>
          new ConfigurationError({
            message: `Failed to read package.json: ${error.message}`,
            userMessage: "Failed to read extension package.json",
          }),
      ),
    );
    const packageJson = yield* Effect.try({
      try: () => JSON.parse(packageJsonContent) as Record<string, unknown>,
      catch: () =>
        new ConfigurationError({
          message: "Invalid JSON in package.json",
          userMessage: "Failed to parse extension package.json",
        }),
    });

    const name = packageJson.name as string;
    const version = packageJson.version as string | undefined;

    // Resolve entry point (uses node:fs internally)
    const { resolveEntryPoint } = yield* Effect.promise(
      () => import("../../core/extension/entry"),
    );

    let entryResolution: ReturnType<typeof resolveEntryPoint>;
    try {
      entryResolution = resolveEntryPoint({
        packageDir: extensionDir,
        packageJson,
      });
    } catch (error) {
      return yield* Effect.fail(
        new ConfigurationError({
          message: `Failed to resolve entry point: ${error instanceof Error ? error.message : String(error)}`,
          userMessage: "Failed to resolve extension entry point",
        }),
      );
    }

    const { entryPath } = entryResolution;

    return yield* bundleExtensionEffect({ name, version }, entryPath, {
      ...options,
      extensionDir,
    });
  });
}

/**
 * Bundles an extension into an ESM artifact with sourcemap.
 */
export function bundleExtensionEffect(
  pkg: ExtensionPackage,
  entryPath: string,
  options: BundleOptions,
): Effect.Effect<BundleResult, ConfigurationError, FileSystem> {
  return Effect.gen(function* () {
    const fs = yield* FileSystem;
    const logger = getLogger();
    const startTime = Date.now();
    const timestamp = options.timestamp || formatTimestamp();

    // Create temp directory structure
    const tempRoot = createTempDirectory(options.repoRoot, timestamp);
    const extensionTempDir = join(tempRoot, pkg.name);

    yield* fs.makeDirectory(extensionTempDir, { recursive: true }).pipe(
      Effect.mapError(
        (error) =>
          new ConfigurationError({
            message: `Failed to create temp directory: ${error.message}`,
            userMessage: "Failed to create temporary build directory",
          }),
      ),
    );

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
    const buildResult = yield* Effect.tryPromise({
      try: () => esbuild.build(config),
      catch: (error) => {
        const errorMessage = error instanceof Error ? error.message : String(error);
        logger.error({
          type: "esbuild_error",
          extension: pkg.name,
          entryPath,
          config,
          error: errorMessage,
        });
        return new ConfigurationError({
          message: `ESBUILD_ERROR: ${errorMessage}`,
          userMessage: `Failed to bundle extension: ${errorMessage}`,
        });
      },
    });

    // Extract output files
    const outputFiles = buildResult.outputFiles;
    if (!outputFiles || outputFiles.length === 0) {
      logger.error({
        type: "bundle_error",
        extension: pkg.name,
        reason: "no_output_files",
        buildResult,
      });
      return yield* Effect.fail(
        new ConfigurationError({
          message: "No output files generated by esbuild",
          userMessage: "Failed to bundle extension: No output files generated by esbuild",
        }),
      );
    }

    const mjsFile = outputFiles.find((f) => f.path.endsWith(".mjs"));
    const mapFile = outputFiles.find((f) => f.path.endsWith(".mjs.map"));

    if (!mjsFile) {
      const fileList = outputFiles.map(f => f.path).join(", ");
      logger.error({
        type: "bundle_error",
        extension: pkg.name,
        reason: "no_mjs_file",
        outputFiles: fileList,
      });
      return yield* Effect.fail(
        new ConfigurationError({
          message: `No .mjs file generated by esbuild. Output files: ${fileList}`,
          userMessage: `Failed to bundle extension: No .mjs file generated. Got: ${fileList}`,
        }),
      );
    }

    let bundleContent = mjsFile.text;

    const bundleWithoutSourceMap = bundleContent
      .replace(/^\/\/# sourceMappingURL=.*$/m, "")
      .trimEnd();

    const sha256 = computeHash(Buffer.from(bundleWithoutSourceMap));
    const hash = shortHash(sha256);

    const artifactName = buildArtifactName(
      pkg.name,
      pkg.version || "0.0.0",
      timestamp,
      hash,
    );

    bundleContent = bundleWithoutSourceMap;
    if (mapFile) {
      const mapName = `${artifactName}.map`;
      bundleContent += `\n//# sourceMappingURL=${mapName}\n`;
    }

    // Write bundle file
    const artifactPath = join(extensionTempDir, artifactName);
    yield* fs.writeFileString(artifactPath, bundleContent);

    // Write sourcemap file if present
    let sourcemapPath: string | undefined;
    if (mapFile) {
      sourcemapPath = `${artifactPath}.map`;
      yield* fs.writeFileString(sourcemapPath, mapFile.text);
    }

    const durationMs = Date.now() - startTime;
    const size = Buffer.byteLength(bundleContent);

    logger.debug({
      type: "bundle",
      extension: pkg.name,
      entry: entryPath,
      size,
      sha256,
      durationMs,
    });

    return {
      packageName: pkg.name,
      version: pkg.version,
      artifactPath,
      artifactName,
      size,
      sha256,
      sourcemapPath,
    };
  }).pipe(
    Effect.catchAll((error) =>
      Effect.fail(
        error instanceof ConfigurationError
          ? error
          : new ConfigurationError({
              message:
                "message" in error ? String(error.message) : String(error),
              userMessage: "Failed to bundle extension",
            }),
      ),
    ),
  );
}
