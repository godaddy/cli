import { mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { findFilesToScan } from "@/core/security/file-discovery.ts";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

describe("File Discovery", () => {
	let testDir: string;

	beforeEach(async () => {
		// Create a unique temp directory for each test
		testDir = join(
			tmpdir(),
			`file-discovery-test-${Date.now()}-${Math.random().toString(36).slice(2)}`,
		);
		await mkdir(testDir, { recursive: true });
	});

	afterEach(async () => {
		// Clean up test directory
		await rm(testDir, { recursive: true, force: true });
	});

	describe("findFilesToScan", () => {
		it("should discover all TypeScript files in a directory", async () => {
			// Create test files
			await writeFile(join(testDir, "index.ts"), "export const a = 1;");
			await writeFile(join(testDir, "utils.ts"), "export const b = 2;");
			await writeFile(join(testDir, "component.tsx"), "export const c = 3;");

			const result = await findFilesToScan(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(3);
			expect(result.data).toEqual(
				expect.arrayContaining([
					join(testDir, "index.ts"),
					join(testDir, "utils.ts"),
					join(testDir, "component.tsx"),
				]),
			);
		});

		it("should discover all JavaScript files in a directory", async () => {
			await writeFile(join(testDir, "index.js"), "module.exports = {};");
			await writeFile(join(testDir, "utils.mjs"), "export const a = 1;");
			await writeFile(join(testDir, "config.cjs"), "module.exports = {};");

			const result = await findFilesToScan(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(3);
			expect(result.data).toEqual(
				expect.arrayContaining([
					join(testDir, "index.js"),
					join(testDir, "utils.mjs"),
					join(testDir, "config.cjs"),
				]),
			);
		});

		it("should discover files in nested directories", async () => {
			const srcDir = join(testDir, "src");
			const libDir = join(srcDir, "lib");
			await mkdir(srcDir, { recursive: true });
			await mkdir(libDir, { recursive: true });

			await writeFile(join(testDir, "index.ts"), "export const a = 1;");
			await writeFile(join(srcDir, "main.ts"), "export const b = 2;");
			await writeFile(join(libDir, "utils.ts"), "export const c = 3;");

			const result = await findFilesToScan(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(3);
			expect(result.data).toEqual(
				expect.arrayContaining([
					join(testDir, "index.ts"),
					join(srcDir, "main.ts"),
					join(libDir, "utils.ts"),
				]),
			);
		});

		it("should exclude node_modules directory", async () => {
			const nodeModulesDir = join(testDir, "node_modules");
			await mkdir(nodeModulesDir, { recursive: true });

			await writeFile(join(testDir, "index.ts"), "export const a = 1;");
			await writeFile(join(nodeModulesDir, "lib.js"), "module.exports = {};");

			const result = await findFilesToScan(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data).toContain(join(testDir, "index.ts"));
			expect(result.data).not.toContain(join(nodeModulesDir, "lib.js"));
		});

		it("should exclude dist directory", async () => {
			const distDir = join(testDir, "dist");
			await mkdir(distDir, { recursive: true });

			await writeFile(join(testDir, "index.ts"), "export const a = 1;");
			await writeFile(join(distDir, "bundle.js"), "// bundled code");

			const result = await findFilesToScan(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data).toContain(join(testDir, "index.ts"));
			expect(result.data).not.toContain(join(distDir, "bundle.js"));
		});

		it("should exclude build directory", async () => {
			const buildDir = join(testDir, "build");
			await mkdir(buildDir, { recursive: true });

			await writeFile(join(testDir, "index.ts"), "export const a = 1;");
			await writeFile(join(buildDir, "output.js"), "// build output");

			const result = await findFilesToScan(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data).toContain(join(testDir, "index.ts"));
			expect(result.data).not.toContain(join(buildDir, "output.js"));
		});

		it("should exclude __tests__ directory", async () => {
			const testsDir = join(testDir, "__tests__");
			await mkdir(testsDir, { recursive: true });

			await writeFile(join(testDir, "index.ts"), "export const a = 1;");
			await writeFile(
				join(testsDir, "index.test.ts"),
				"test('example', () => {});",
			);

			const result = await findFilesToScan(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data).toContain(join(testDir, "index.ts"));
			expect(result.data).not.toContain(join(testsDir, "index.test.ts"));
		});

		it("should exclude nested excluded directories", async () => {
			const srcDir = join(testDir, "src");
			const nodeModulesDir = join(srcDir, "node_modules");
			await mkdir(srcDir, { recursive: true });
			await mkdir(nodeModulesDir, { recursive: true });

			await writeFile(join(testDir, "index.ts"), "export const a = 1;");
			await writeFile(join(srcDir, "main.ts"), "export const b = 2;");
			await writeFile(join(nodeModulesDir, "lib.js"), "module.exports = {};");

			const result = await findFilesToScan(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(2);
			expect(result.data).toEqual(
				expect.arrayContaining([
					join(testDir, "index.ts"),
					join(srcDir, "main.ts"),
				]),
			);
			expect(result.data).not.toContain(join(nodeModulesDir, "lib.js"));
		});

		it("should ignore non-source files", async () => {
			await writeFile(join(testDir, "index.ts"), "export const a = 1;");
			await writeFile(join(testDir, "README.md"), "# README");
			await writeFile(join(testDir, "config.json"), "{}");
			await writeFile(join(testDir, "data.txt"), "some data");

			const result = await findFilesToScan(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data).toContain(join(testDir, "index.ts"));
		});

		it("should return empty array for directory with no source files", async () => {
			await writeFile(join(testDir, "README.md"), "# README");
			await writeFile(join(testDir, "config.json"), "{}");

			const result = await findFilesToScan(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(0);
		});

		it("should handle empty directory", async () => {
			const result = await findFilesToScan(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(0);
		});

		it("should return error for non-existent directory", async () => {
			const nonExistentDir = join(testDir, "does-not-exist");

			const result = await findFilesToScan(nonExistentDir);

			expect(result.success).toBe(false);
			expect(result.error).toBeDefined();
			expect(result.error?.message).toContain("ENOENT");
		});

		it("should discover mixed file types", async () => {
			await writeFile(join(testDir, "index.ts"), "export const a = 1;");
			await writeFile(join(testDir, "utils.js"), "module.exports = {};");
			await writeFile(join(testDir, "component.tsx"), "export const b = 2;");
			await writeFile(join(testDir, "helper.jsx"), "export const c = 3;");
			await writeFile(join(testDir, "module.mjs"), "export const d = 4;");
			await writeFile(join(testDir, "config.cjs"), "module.exports = {};");

			const result = await findFilesToScan(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(6);
		});

		it("should return absolute paths", async () => {
			await writeFile(join(testDir, "index.ts"), "export const a = 1;");

			const result = await findFilesToScan(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data?.[0]).toMatch(/^[/\\]/); // Starts with / or \ (absolute path)
		});

		it("should handle relative paths in input", async () => {
			await writeFile(join(testDir, "index.ts"), "export const a = 1;");

			// Pass a relative path
			const result = await findFilesToScan(".");

			expect(result.success).toBe(true);
			expect(result.data).toBeDefined();
			// All returned paths should be absolute
			if (result.data && result.data.length > 0) {
				expect(result.data[0]).toMatch(/^[/\\]/);
			}
		});

		it("should exclude multiple patterns simultaneously", async () => {
			// Create multiple excluded directories
			const nodeModulesDir = join(testDir, "node_modules");
			const distDir = join(testDir, "dist");
			const buildDir = join(testDir, "build");
			const testsDir = join(testDir, "__tests__");
			const srcDir = join(testDir, "src");

			await mkdir(nodeModulesDir, { recursive: true });
			await mkdir(distDir, { recursive: true });
			await mkdir(buildDir, { recursive: true });
			await mkdir(testsDir, { recursive: true });
			await mkdir(srcDir, { recursive: true });

			await writeFile(join(testDir, "index.ts"), "export const a = 1;");
			await writeFile(join(srcDir, "main.ts"), "export const b = 2;");
			await writeFile(join(nodeModulesDir, "lib.js"), "module.exports = {};");
			await writeFile(join(distDir, "bundle.js"), "// bundled");
			await writeFile(join(buildDir, "output.js"), "// build");
			await writeFile(join(testsDir, "test.ts"), "test('example', () => {});");

			const result = await findFilesToScan(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(2);
			expect(result.data).toEqual(
				expect.arrayContaining([
					join(testDir, "index.ts"),
					join(srcDir, "main.ts"),
				]),
			);
		});
	});
});
