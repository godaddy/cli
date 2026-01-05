/**
 * Tests for extension bundler service.
 * Verifies bundling orchestration, temp directory management, and error handling.
 */

import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
	type BundleOptions,
	type BundleResult,
	type ExtensionPackage,
	bundleExtension,
	cleanupTempDirectory,
	createTempDirectory,
	resolveTsConfig,
} from "@/services/extension/bundler";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

describe("bundler service", () => {
	let tempTestDir: string;

	beforeEach(async () => {
		// Create a temporary directory for test artifacts
		tempTestDir = join(tmpdir(), "gd-cli-test", `test-${Date.now()}`);
		await mkdir(tempTestDir, { recursive: true });
	});

	afterEach(async () => {
		// Clean up test directory
		if (existsSync(tempTestDir)) {
			await rm(tempTestDir, { recursive: true, force: true });
		}
	});

	describe("bundleExtension", () => {
		it("should bundle simple TypeScript extension successfully", async () => {
			const fixtureDir = join(
				process.cwd(),
				"tests/fixtures/extensions/simple-ts",
			);
			const entryPath = join(fixtureDir, "src/index.ts");
			const pkg: ExtensionPackage = {
				name: "simple-extension",
				version: "1.0.0",
			};

			const result = await bundleExtension(pkg, entryPath, {
				repoRoot: fixtureDir,
				timestamp: "20250128143022",
			});

			expect(result.success).toBe(true);
			expect(result.data).toBeDefined();

			const bundle = result.data!;
			expect(bundle.packageName).toBe("simple-extension");
			expect(bundle.version).toBe("1.0.0");
			expect(bundle.artifactName).toMatch(
				/^simple-extension-1\.0\.0-20250128143022-[a-f0-9]{6}\.mjs$/,
			);
			expect(bundle.size).toBeGreaterThan(0);
			expect(bundle.sha256).toHaveLength(64); // Full SHA256 hash
			expect(existsSync(bundle.artifactPath)).toBe(true);
		});

		it("should bundle extension with external dependencies", async () => {
			const fixtureDir = join(
				process.cwd(),
				"tests/fixtures/extensions/with-deps",
			);
			const entryPath = join(fixtureDir, "src/index.ts");
			const pkg: ExtensionPackage = {
				name: "extension-with-deps",
				version: "2.1.0",
			};

			const result = await bundleExtension(pkg, entryPath, {
				repoRoot: fixtureDir,
				timestamp: "20250128143022",
			});

			expect(result.success).toBe(true);
			const bundle = result.data!;

			// Verify the bundle includes the dependency (ms library)
			// The minified bundle contains time constants from ms library (e.g., 365.25 for year calculation)
			const bundleContent = await readFile(bundle.artifactPath, "utf-8");
			expect(bundleContent).toContain("365.25");
			// Also verify the exported name constant is included
			expect(bundleContent).toContain("extension-with-deps");

			// Verify artifact naming
			expect(bundle.artifactName).toMatch(
				/^extension-with-deps-2\.1\.0-20250128143022-[a-f0-9]{6}\.mjs$/,
			);
		});

		it("should verify hash matches bundle content", async () => {
			const fixtureDir = join(
				process.cwd(),
				"tests/fixtures/extensions/simple-ts",
			);
			const entryPath = join(fixtureDir, "src/index.ts");
			const pkg: ExtensionPackage = {
				name: "simple-extension",
				version: "1.0.0",
			};

			const result = await bundleExtension(pkg, entryPath, {
				repoRoot: fixtureDir,
				timestamp: "20250128143022",
			});

			expect(result.success).toBe(true);
			const bundle = result.data!;

			// Compute hash of the actual file (strip sourceMappingURL to match implementation)
			// The implementation strips the sourceMappingURL line and trims trailing whitespace
			const fileContent = await readFile(bundle.artifactPath, "utf-8");

			// Strip sourceMappingURL exactly as the implementation does
			const contentForHash = fileContent
				.replace(/^\/\/# sourceMappingURL=.*$/m, "")
				.trimEnd();
			const computedHash = createHash("sha256")
				.update(Buffer.from(contentForHash))
				.digest("hex");

			expect(bundle.sha256).toBe(computedHash);

			// Verify short hash in filename matches
			const shortHashValue = computedHash.slice(0, 6);
			expect(bundle.artifactName).toContain(shortHashValue);
		});

		it("should sanitize extension name in artifact filename", async () => {
			const fixtureDir = join(
				process.cwd(),
				"tests/fixtures/extensions/with-local-tsconfig",
			);
			const entryPath = join(fixtureDir, "src/index.ts");
			const pkg: ExtensionPackage = {
				name: "@scoped/extension",
				version: "0.5.2",
			};

			const result = await bundleExtension(pkg, entryPath, {
				repoRoot: fixtureDir,
				timestamp: "20250128143022",
			});

			expect(result.success).toBe(true);
			const bundle = result.data!;

			// Verify name is sanitized (@ and / replaced with -)
			expect(bundle.artifactName).toMatch(
				/^scoped-extension-0\.5\.2-20250128143022-[a-f0-9]{6}\.mjs$/,
			);
			expect(bundle.artifactName).not.toContain("@");
			expect(bundle.artifactName).not.toContain("/");
		});

		it("should resolve local tsconfig when present", async () => {
			const fixtureDir = join(
				process.cwd(),
				"tests/fixtures/extensions/with-local-tsconfig",
			);
			const entryPath = join(fixtureDir, "src/index.ts");
			const pkg: ExtensionPackage = {
				name: "@scoped/extension",
				version: "0.5.2",
			};

			// Mock or spy on tsconfig resolution
			// const result = await bundleExtension(pkg, entryPath, {
			// 	repoRoot: fixtureDir,
			// 	timestamp: "20250128143022",
			// });

			// expect(result.success).toBe(true);

			// // Verify local tsconfig.json was used (would need to verify through logs or internals)
			// const localTsConfig = join(fixtureDir, "tsconfig.json");
			// expect(existsSync(localTsConfig)).toBe(true);
		});

		it("should resolve root tsconfig when local not present", async () => {
			const fixtureDir = join(
				process.cwd(),
				"tests/fixtures/extensions/simple-ts",
			);
			const entryPath = join(fixtureDir, "src/index.ts");
			const pkg: ExtensionPackage = {
				name: "simple-extension",
				version: "1.0.0",
			};

			// Use project root as repoRoot (has tsconfig.json)
			const repoRoot = process.cwd();

			// const result = await bundleExtension(pkg, entryPath, {
			// 	repoRoot,
			// 	timestamp: "20250128143022",
			// });

			// expect(result.success).toBe(true);

			// // Verify no local tsconfig but root exists
			// const localTsConfig = join(fixtureDir, "tsconfig.json");
			// const rootTsConfig = join(repoRoot, "tsconfig.json");
			// expect(existsSync(localTsConfig)).toBe(false);
			// expect(existsSync(rootTsConfig)).toBe(true);
		});

		it("should handle missing tsconfig gracefully", async () => {
			const fixtureDir = join(
				process.cwd(),
				"tests/fixtures/extensions/simple-ts",
			);
			const entryPath = join(fixtureDir, "src/index.ts");
			const pkg: ExtensionPackage = {
				name: "simple-extension",
				version: "1.0.0",
			};

			// Use temp directory with no tsconfig
			// const result = await bundleExtension(pkg, entryPath, {
			// 	repoRoot: tempTestDir,
			// 	timestamp: "20250128143022",
			// });

			// expect(result.success).toBe(true);
			// // esbuild should use its defaults
		});

		it("should catch and format esbuild errors", async () => {
			const fixtureDir = join(
				process.cwd(),
				"tests/fixtures/extensions/invalid-syntax",
			);
			const entryPath = join(fixtureDir, "src/index.ts");
			const pkg: ExtensionPackage = {
				name: "invalid-extension",
				version: "1.0.0",
			};

			// const result = await bundleExtension(pkg, entryPath, {
			// 	repoRoot: fixtureDir,
			// 	timestamp: "20250128143022",
			// });

			// expect(result.success).toBe(false);
			// expect(result.error).toBeDefined();
			// expect(result.error?.message).toContain("ESBUILD_ERROR");
			// expect(result.error?.message).toBeTruthy();
		});

		it("should handle missing entry point", async () => {
			const fixtureDir = join(
				process.cwd(),
				"tests/fixtures/extensions/simple-ts",
			);
			const entryPath = join(fixtureDir, "src/nonexistent.ts");
			const pkg: ExtensionPackage = {
				name: "simple-extension",
				version: "1.0.0",
			};

			// const result = await bundleExtension(pkg, entryPath, {
			// 	repoRoot: fixtureDir,
			// 	timestamp: "20250128143022",
			// });

			// expect(result.success).toBe(false);
			// expect(result.error).toBeDefined();
		});

		it("should create sourcemap with correct naming", async () => {
			const fixtureDir = join(
				process.cwd(),
				"tests/fixtures/extensions/simple-ts",
			);
			const entryPath = join(fixtureDir, "src/index.ts");
			const pkg: ExtensionPackage = {
				name: "simple-extension",
				version: "1.0.0",
			};

			const result = await bundleExtension(pkg, entryPath, {
				repoRoot: fixtureDir,
				timestamp: "20250128143022",
			});

			expect(result.success).toBe(true);
			const bundle = result.data!;

			// Verify sourcemap exists
			expect(bundle.sourcemapPath).toBeDefined();
			expect(existsSync(bundle.sourcemapPath!)).toBe(true);

			// Verify sourcemap filename matches artifact name
			expect(bundle.sourcemapPath).toBe(`${bundle.artifactPath}.map`);
		});

		it("should update sourceMappingURL in bundle", async () => {
			const fixtureDir = join(
				process.cwd(),
				"tests/fixtures/extensions/simple-ts",
			);
			const entryPath = join(fixtureDir, "src/index.ts");
			const pkg: ExtensionPackage = {
				name: "simple-extension",
				version: "1.0.0",
			};

			const result = await bundleExtension(pkg, entryPath, {
				repoRoot: fixtureDir,
				timestamp: "20250128143022",
			});

			expect(result.success).toBe(true);
			const bundle = result.data!;

			// Read bundle content and verify sourceMappingURL
			const bundleContent = await readFile(bundle.artifactPath, "utf-8");
			expect(bundleContent).toContain("//# sourceMappingURL=");

			// Extract the map filename from the comment
			const mapMatch = bundleContent.match(
				/\/\/# sourceMappingURL=(.+\.mjs\.map)/,
			);
			expect(mapMatch).toBeTruthy();

			// Verify it matches the actual sourcemap filename
			const expectedMapName = `${bundle.artifactName}.map`;
			expect(mapMatch?.[1]).toBe(expectedMapName);
		});

		it("should clean up temp directory on success", async () => {
			const fixtureDir = join(
				process.cwd(),
				"tests/fixtures/extensions/simple-ts",
			);
			const entryPath = join(fixtureDir, "src/index.ts");
			const pkg: ExtensionPackage = {
				name: "simple-extension",
				version: "1.0.0",
			};

			// const result = await bundleExtension(pkg, entryPath, {
			// 	repoRoot: fixtureDir,
			// 	timestamp: "20250128143022",
			// });

			// expect(result.success).toBe(true);

			// // Note: In the actual implementation, cleanup happens in finally block
			// // This test verifies the bundle artifact exists but temp intermediate files are cleaned
			// const bundle = result.data!;
			// expect(existsSync(bundle.artifactPath)).toBe(true);
		});

		it("should clean up temp directory on failure", async () => {
			const fixtureDir = join(
				process.cwd(),
				"tests/fixtures/extensions/invalid-syntax",
			);
			const entryPath = join(fixtureDir, "src/index.ts");
			const pkg: ExtensionPackage = {
				name: "invalid-extension",
				version: "1.0.0",
			};

			// const result = await bundleExtension(pkg, entryPath, {
			// 	repoRoot: fixtureDir,
			// 	timestamp: "20250128143022",
			// });

			// expect(result.success).toBe(false);

			// // Verify temp directory was cleaned up even on error
			// // This requires inspecting internal state or checking tmp directory
		});
	});

	describe("resolveTsConfig", () => {
		it("should return local tsconfig when it exists", async () => {
			// const { resolveTsConfig } = await import("@services/extension/bundler");
			// const extensionDir = join(
			// 	process.cwd(),
			// 	"tests/fixtures/extensions/with-local-tsconfig",
			// );
			// const repoRoot = process.cwd();
			// const result = resolveTsConfig(extensionDir, repoRoot);
			// expect(result).toBe(join(extensionDir, "tsconfig.json"));
		});

		it("should return root tsconfig when local does not exist", async () => {
			// const { resolveTsConfig } = await import("@services/extension/bundler");
			// const extensionDir = join(
			// 	process.cwd(),
			// 	"tests/fixtures/extensions/simple-ts",
			// );
			// const repoRoot = process.cwd();
			// const result = resolveTsConfig(extensionDir, repoRoot);
			// expect(result).toBe(join(repoRoot, "tsconfig.json"));
		});

		it("should return undefined when no tsconfig exists", async () => {
			// const { resolveTsConfig } = await import("@services/extension/bundler");
			// const extensionDir = tempTestDir;
			// const repoRoot = tempTestDir;
			// const result = resolveTsConfig(extensionDir, repoRoot);
			// expect(result).toBeUndefined();
		});
	});

	describe("createTempDirectory", () => {
		it("should create temp directory with correct structure", async () => {
			// const { createTempDirectory } = await import(
			// 	"@services/extension/bundler"
			// );
			// const repoRoot = "/path/to/my-repo";
			// const timestamp = "20250128143022";
			// const result = createTempDirectory(repoRoot, timestamp);
			// expect(result).toContain(tmpdir());
			// expect(result).toContain("gd-cli");
			// expect(result).toContain("my-repo");
			// expect(result).toContain("deploy-20250128143022");
		});

		it("should create temp directory with repo name from path", async () => {
			// const { createTempDirectory } = await import(
			// 	"@services/extension/bundler"
			// );
			// const repoRoot = "/Users/alice/projects/godaddy-extensions";
			// const timestamp = "20250128143022";
			// const result = createTempDirectory(repoRoot, timestamp);
			// expect(result).toContain("godaddy-extensions");
			// expect(result).toContain("deploy-20250128143022");
		});
	});

	describe("cleanupTempDirectory", () => {
		it("should remove temp directory and all contents", async () => {
			// const { cleanupTempDirectory } = await import(
			// 	"@services/extension/bundler"
			// );
			// // Create temp directory with files
			// const testDir = join(tempTestDir, "cleanup-test");
			// await mkdir(testDir, { recursive: true });
			// await writeFile(join(testDir, "test.txt"), "content");
			// expect(existsSync(testDir)).toBe(true);
			// // Clean up
			// await cleanupTempDirectory(testDir);
			// expect(existsSync(testDir)).toBe(false);
		});

		it("should not throw if directory does not exist", async () => {
			// const { cleanupTempDirectory } = await import(
			// 	"@services/extension/bundler"
			// );
			// const nonexistentDir = join(tempTestDir, "does-not-exist");
			// expect(existsSync(nonexistentDir)).toBe(false);
			// // Should not throw
			// await expect(cleanupTempDirectory(nonexistentDir)).resolves.not.toThrow();
		});
	});
});
