import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import {
	detectPackageManager,
	getExtensions,
	readPackageJson,
} from "../../../src/services/extension/workspace";

describe("Extension Workspace", () => {
	let tempDir: string;

	beforeEach(() => {
		// Create a temporary directory for each test
		tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "godaddy-cli-test-"));
	});

	afterEach(() => {
		// Clean up temporary directory
		if (fs.existsSync(tempDir)) {
			fs.rmSync(tempDir, { recursive: true, force: true });
		}
	});

	describe("readPackageJson", () => {
		test("successfully reads and parses valid package.json", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			const packageData = {
				name: "test-package",
				version: "1.0.0",
				type: "module",
			};
			fs.writeFileSync(packageJsonPath, JSON.stringify(packageData, null, 2));

			const result = readPackageJson(packageJsonPath);

			expect(result.success).toBe(true);
			expect(result.data).toEqual(packageData);
		});

		test("returns error when package.json does not exist", () => {
			const packageJsonPath = path.join(tempDir, "nonexistent.json");

			const result = readPackageJson(packageJsonPath);

			expect(result.success).toBe(false);
			expect(result.error?.message).toContain("not found");
		});

		test("returns error when package.json has invalid JSON", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			fs.writeFileSync(packageJsonPath, "{ invalid json }");

			const result = readPackageJson(packageJsonPath);

			expect(result.success).toBe(false);
			expect(result.error?.message).toContain("Failed to parse");
		});
	});

	describe("detectPackageManager", () => {
		test("detects pnpm from package.json packageManager field", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			fs.writeFileSync(
				packageJsonPath,
				JSON.stringify({ packageManager: "pnpm@8.0.0" }),
			);

			const result = detectPackageManager(tempDir);

			expect(result).toBe("pnpm");
		});

		test("detects yarn from package.json packageManager field", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			fs.writeFileSync(
				packageJsonPath,
				JSON.stringify({ packageManager: "yarn@3.0.0" }),
			);

			const result = detectPackageManager(tempDir);

			expect(result).toBe("yarn");
		});

		test("detects npm from package.json packageManager field", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			fs.writeFileSync(
				packageJsonPath,
				JSON.stringify({ packageManager: "npm@9.0.0" }),
			);

			const result = detectPackageManager(tempDir);

			expect(result).toBe("npm");
		});

		test("falls back to pnpm-lock.yaml detection", () => {
			fs.writeFileSync(path.join(tempDir, "pnpm-lock.yaml"), "");

			const result = detectPackageManager(tempDir);

			expect(result).toBe("pnpm");
		});

		test("falls back to yarn.lock detection", () => {
			fs.writeFileSync(path.join(tempDir, "yarn.lock"), "");

			const result = detectPackageManager(tempDir);

			expect(result).toBe("yarn");
		});

		test("falls back to package-lock.json detection", () => {
			fs.writeFileSync(path.join(tempDir, "package-lock.json"), "");

			const result = detectPackageManager(tempDir);

			expect(result).toBe("npm");
		});

		test("returns unknown when no package manager is detected", () => {
			const result = detectPackageManager(tempDir);

			expect(result).toBe("unknown");
		});

		test("prioritizes packageManager field over lockfiles", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			fs.writeFileSync(
				packageJsonPath,
				JSON.stringify({ packageManager: "yarn@3.0.0" }),
			);
			fs.writeFileSync(path.join(tempDir, "pnpm-lock.yaml"), "");

			const result = detectPackageManager(tempDir);

			expect(result).toBe("yarn");
		});
	});

	describe("getExtensions", () => {
		test("returns error when extensions directory does not exist and no workspaces", async () => {
			const result = await getExtensions({ repoRoot: tempDir });

			expect(result.success).toBe(false);
			expect(result.error?.message).toContain("no package.json found");
		});

		test("returns error when extensions path is a file, not a directory", async () => {
			const extensionsPath = path.join(tempDir, "extensions");
			fs.writeFileSync(extensionsPath, "not a directory");

			const result = await getExtensions({ repoRoot: tempDir });

			expect(result.success).toBe(false);
			expect(result.error?.message).toContain("not a directory");
		});

		test("returns empty array when extensions directory is empty", async () => {
			const extensionsPath = path.join(tempDir, "extensions");
			fs.mkdirSync(extensionsPath);

			const result = await getExtensions({ repoRoot: tempDir });

			expect(result.success).toBe(true);
			expect(result.data).toEqual([]);
		});

		test("finds single extension with valid package.json", async () => {
			const extensionsPath = path.join(tempDir, "extensions");
			const ext1Path = path.join(extensionsPath, "ext1");
			fs.mkdirSync(ext1Path, { recursive: true });

			const packageJson = {
				name: "@test/ext1",
				version: "1.0.0",
			};
			fs.writeFileSync(
				path.join(ext1Path, "package.json"),
				JSON.stringify(packageJson),
			);

			const result = await getExtensions({ repoRoot: tempDir });

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data?.[0]).toMatchObject({
				name: "@test/ext1",
				version: "1.0.0",
				dir: ext1Path,
				packageManager: "unknown",
			});
		});

		test("finds multiple extensions", async () => {
			const extensionsPath = path.join(tempDir, "extensions");
			fs.mkdirSync(extensionsPath);

			// Create ext1
			const ext1Path = path.join(extensionsPath, "ext1");
			fs.mkdirSync(ext1Path);
			fs.writeFileSync(
				path.join(ext1Path, "package.json"),
				JSON.stringify({ name: "ext1", version: "1.0.0" }),
			);

			// Create ext2
			const ext2Path = path.join(extensionsPath, "ext2");
			fs.mkdirSync(ext2Path);
			fs.writeFileSync(
				path.join(ext2Path, "package.json"),
				JSON.stringify({ name: "ext2", version: "2.0.0" }),
			);

			const result = await getExtensions({ repoRoot: tempDir });

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(2);
			expect(result.data?.map((e) => e.name)).toContain("ext1");
			expect(result.data?.map((e) => e.name)).toContain("ext2");
		});

		test("skips directories without package.json", async () => {
			const extensionsPath = path.join(tempDir, "extensions");
			fs.mkdirSync(extensionsPath);

			// Create directory with package.json
			const ext1Path = path.join(extensionsPath, "ext1");
			fs.mkdirSync(ext1Path);
			fs.writeFileSync(
				path.join(ext1Path, "package.json"),
				JSON.stringify({ name: "ext1" }),
			);

			// Create directory without package.json
			const ext2Path = path.join(extensionsPath, "ext2");
			fs.mkdirSync(ext2Path);

			const result = await getExtensions({ repoRoot: tempDir });

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data?.[0].name).toBe("ext1");
		});

		test("skips files in extensions directory", async () => {
			const extensionsPath = path.join(tempDir, "extensions");
			fs.mkdirSync(extensionsPath);

			// Create a file in extensions directory
			fs.writeFileSync(path.join(extensionsPath, "README.md"), "# Extensions");

			// Create valid extension
			const ext1Path = path.join(extensionsPath, "ext1");
			fs.mkdirSync(ext1Path);
			fs.writeFileSync(
				path.join(ext1Path, "package.json"),
				JSON.stringify({ name: "ext1" }),
			);

			const result = await getExtensions({ repoRoot: tempDir });

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
		});

		test("returns error when package.json is missing name field", async () => {
			const extensionsPath = path.join(tempDir, "extensions");
			const ext1Path = path.join(extensionsPath, "ext1");
			fs.mkdirSync(ext1Path, { recursive: true });

			fs.writeFileSync(
				path.join(ext1Path, "package.json"),
				JSON.stringify({ version: "1.0.0" }),
			);

			const result = await getExtensions({ repoRoot: tempDir });

			expect(result.success).toBe(false);
			expect(result.error?.message).toContain('missing "name" field');
		});

		test("returns error when package.json is invalid JSON", async () => {
			const extensionsPath = path.join(tempDir, "extensions");
			const ext1Path = path.join(extensionsPath, "ext1");
			fs.mkdirSync(ext1Path, { recursive: true });

			fs.writeFileSync(path.join(ext1Path, "package.json"), "{ invalid }");

			const result = await getExtensions({ repoRoot: tempDir });

			expect(result.success).toBe(false);
			expect(result.error?.message).toContain("Failed to read package.json");
		});

		test("uses custom extensions directory", async () => {
			const customDir = path.join(tempDir, "my-extensions");
			const ext1Path = path.join(customDir, "ext1");
			fs.mkdirSync(ext1Path, { recursive: true });

			fs.writeFileSync(
				path.join(ext1Path, "package.json"),
				JSON.stringify({ name: "ext1" }),
			);

			const result = await getExtensions({
				repoRoot: tempDir,
				extensionsDir: "my-extensions",
			});

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
		});

		test("detects package manager for extensions", async () => {
			const extensionsPath = path.join(tempDir, "extensions");
			const ext1Path = path.join(extensionsPath, "ext1");
			fs.mkdirSync(ext1Path, { recursive: true });

			// Create pnpm-lock.yaml at repo root
			fs.writeFileSync(path.join(tempDir, "pnpm-lock.yaml"), "");

			fs.writeFileSync(
				path.join(ext1Path, "package.json"),
				JSON.stringify({ name: "ext1" }),
			);

			const result = await getExtensions({ repoRoot: tempDir });

			expect(result.success).toBe(true);
			expect(result.data?.[0].packageManager).toBe("pnpm");
		});
	});

	describe("getExtensions - workspaces fallback", () => {
		test("falls back to workspaces when extensions directory does not exist", async () => {
			// Create root package.json with workspaces
			const rootPackageJson = {
				name: "root",
				workspaces: ["packages/*"],
			};
			fs.writeFileSync(
				path.join(tempDir, "package.json"),
				JSON.stringify(rootPackageJson),
			);

			// Create workspace package marked as extension
			const packagesDir = path.join(tempDir, "packages");
			const pkg1Dir = path.join(packagesDir, "pkg1");
			fs.mkdirSync(pkg1Dir, { recursive: true });
			fs.writeFileSync(
				path.join(pkg1Dir, "package.json"),
				JSON.stringify({
					name: "@test/pkg1",
					version: "1.0.0",
					godaddy: { extension: true },
				}),
			);

			const result = await getExtensions({ repoRoot: tempDir });

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data?.[0].name).toBe("@test/pkg1");
		});

		test("uses godaddy.type = 'extension' to mark workspace as extension", async () => {
			const rootPackageJson = {
				workspaces: ["packages/*"],
			};
			fs.writeFileSync(
				path.join(tempDir, "package.json"),
				JSON.stringify(rootPackageJson),
			);

			const packagesDir = path.join(tempDir, "packages");
			const pkg1Dir = path.join(packagesDir, "pkg1");
			fs.mkdirSync(pkg1Dir, { recursive: true });
			fs.writeFileSync(
				path.join(pkg1Dir, "package.json"),
				JSON.stringify({
					name: "pkg1",
					godaddy: { type: "extension" },
				}),
			);

			const result = await getExtensions({ repoRoot: tempDir });

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data?.[0].name).toBe("pkg1");
		});

		test("skips workspace packages not marked as extensions", async () => {
			const rootPackageJson = {
				workspaces: ["packages/*"],
			};
			fs.writeFileSync(
				path.join(tempDir, "package.json"),
				JSON.stringify(rootPackageJson),
			);

			const packagesDir = path.join(tempDir, "packages");

			// Extension package
			const ext1Dir = path.join(packagesDir, "ext1");
			fs.mkdirSync(ext1Dir, { recursive: true });
			fs.writeFileSync(
				path.join(ext1Dir, "package.json"),
				JSON.stringify({
					name: "ext1",
					godaddy: { extension: true },
				}),
			);

			// Regular package (not an extension)
			const pkg1Dir = path.join(packagesDir, "pkg1");
			fs.mkdirSync(pkg1Dir, { recursive: true });
			fs.writeFileSync(
				path.join(pkg1Dir, "package.json"),
				JSON.stringify({ name: "pkg1" }),
			);

			const result = await getExtensions({ repoRoot: tempDir });

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data?.[0].name).toBe("ext1");
		});

		test("handles direct workspace paths (non-glob)", async () => {
			const rootPackageJson = {
				workspaces: ["packages/ext1"],
			};
			fs.writeFileSync(
				path.join(tempDir, "package.json"),
				JSON.stringify(rootPackageJson),
			);

			const ext1Dir = path.join(tempDir, "packages", "ext1");
			fs.mkdirSync(ext1Dir, { recursive: true });
			fs.writeFileSync(
				path.join(ext1Dir, "package.json"),
				JSON.stringify({
					name: "ext1",
					godaddy: { extension: true },
				}),
			);

			const result = await getExtensions({ repoRoot: tempDir });

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
		});

		test("returns error when no extensions directory and no package.json", async () => {
			const result = await getExtensions({ repoRoot: tempDir });

			expect(result.success).toBe(false);
			expect(result.error?.message).toContain("no package.json found");
		});

		test("returns error when no extensions directory and no workspaces", async () => {
			fs.writeFileSync(
				path.join(tempDir, "package.json"),
				JSON.stringify({ name: "root" }),
			);

			const result = await getExtensions({ repoRoot: tempDir });

			expect(result.success).toBe(false);
			expect(result.error?.message).toContain("no workspaces defined");
		});

		test("skips workspace packages with invalid package.json", async () => {
			const rootPackageJson = {
				workspaces: ["packages/*"],
			};
			fs.writeFileSync(
				path.join(tempDir, "package.json"),
				JSON.stringify(rootPackageJson),
			);

			const packagesDir = path.join(tempDir, "packages");

			// Valid extension
			const ext1Dir = path.join(packagesDir, "ext1");
			fs.mkdirSync(ext1Dir, { recursive: true });
			fs.writeFileSync(
				path.join(ext1Dir, "package.json"),
				JSON.stringify({
					name: "ext1",
					godaddy: { extension: true },
				}),
			);

			// Invalid package.json
			const ext2Dir = path.join(packagesDir, "ext2");
			fs.mkdirSync(ext2Dir, { recursive: true });
			fs.writeFileSync(path.join(ext2Dir, "package.json"), "{ invalid }");

			const result = await getExtensions({ repoRoot: tempDir });

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data?.[0].name).toBe("ext1");
		});

		test("skips workspace packages without name field", async () => {
			const rootPackageJson = {
				workspaces: ["packages/*"],
			};
			fs.writeFileSync(
				path.join(tempDir, "package.json"),
				JSON.stringify(rootPackageJson),
			);

			const packagesDir = path.join(tempDir, "packages");

			// Extension without name
			const ext1Dir = path.join(packagesDir, "ext1");
			fs.mkdirSync(ext1Dir, { recursive: true });
			fs.writeFileSync(
				path.join(ext1Dir, "package.json"),
				JSON.stringify({
					version: "1.0.0",
					godaddy: { extension: true },
				}),
			);

			const result = await getExtensions({ repoRoot: tempDir });

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(0);
		});

		test("prioritizes extensions directory over workspaces", async () => {
			// Create both extensions directory and workspaces
			const extensionsPath = path.join(tempDir, "extensions");
			const ext1Path = path.join(extensionsPath, "ext1");
			fs.mkdirSync(ext1Path, { recursive: true });
			fs.writeFileSync(
				path.join(ext1Path, "package.json"),
				JSON.stringify({ name: "ext-from-dir" }),
			);

			const rootPackageJson = {
				workspaces: ["packages/*"],
			};
			fs.writeFileSync(
				path.join(tempDir, "package.json"),
				JSON.stringify(rootPackageJson),
			);

			const packagesDir = path.join(tempDir, "packages");
			const pkg1Dir = path.join(packagesDir, "pkg1");
			fs.mkdirSync(pkg1Dir, { recursive: true });
			fs.writeFileSync(
				path.join(pkg1Dir, "package.json"),
				JSON.stringify({
					name: "ext-from-workspace",
					godaddy: { extension: true },
				}),
			);

			const result = await getExtensions({ repoRoot: tempDir });

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data?.[0].name).toBe("ext-from-dir");
		});
	});
});
