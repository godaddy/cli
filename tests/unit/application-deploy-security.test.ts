import { mkdir, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { applicationDeploy } from "../../src/core/applications";
import * as authModule from "../../src/core/auth";
import * as applicationsService from "../../src/services/applications";
import * as configService from "../../src/services/config";
import type { ConfigExtensionInfo } from "../../src/services/config";
import * as presignedUrlService from "../../src/services/extension/presigned-url";
import * as uploadService from "../../src/services/extension/upload";

const testDir = join(process.cwd(), "tests", "fixtures", "test-app-deploy");
const extensionsDir = join(testDir, "extensions");

describe("Application Deploy with Security Scanning", () => {
	let getExtensionsFromConfigSpy: ReturnType<typeof vi.spyOn>;

	beforeEach(async () => {
		// Clean up and create test directory with extensions folder
		await rm(testDir, { recursive: true, force: true });
		await mkdir(extensionsDir, { recursive: true });

		// Mock authentication
		vi.spyOn(authModule, "getFromKeychain").mockResolvedValue("test-token");

		// Mock getApplicationAndLatestRelease to return a valid application with release
		vi.spyOn(
			applicationsService,
			"getApplicationAndLatestRelease",
		).mockResolvedValue({
			application: {
				id: "app-123",
				name: "test-app",
				label: "Test App",
				description: "Test application",
				status: "INACTIVE",
				url: "https://test.example.com",
				proxyUrl: "https://proxy.test.example.com",
				authorizationScopes: ["scope1"],
				releases: {
					edges: [
						{
							node: {
								id: "release-123",
								version: "1.0.0",
								description: "Initial release",
								createdAt: "2025-01-01T00:00:00Z",
							},
						},
					],
				},
			},
		});

		// Mock updateApplication
		vi.spyOn(applicationsService, "updateApplication").mockResolvedValue({
			updateApplication: {
				id: "app-123",
				name: "test-app",
				label: "Test App",
				description: "Test application",
				status: "ACTIVE",
				url: "https://test.example.com",
				proxyUrl: "https://proxy.test.example.com",
				clientId: "test-client-id",
			},
		});

		// Setup spy for getExtensionsFromConfig (will be configured per test)
		getExtensionsFromConfigSpy = vi.spyOn(
			configService,
			"getExtensionsFromConfig",
		);

		// Mock presigned URL generation
		vi.spyOn(presignedUrlService, "getUploadTarget").mockResolvedValue({
			uploadId: "upload-123",
			url: "https://s3.example.com/presigned-url",
			key: "test/bundle.js",
			expiresAt: "2025-12-31T23:59:59Z",
			maxSizeBytes: 10485760,
			requiredHeaders: {},
		});

		// Mock S3 upload
		vi.spyOn(uploadService, "uploadArtifact").mockResolvedValue({
			uploadId: "upload-123",
			etag: '"abc123"',
			status: 200,
			sizeBytes: 1000,
		});
	});

	afterEach(async () => {
		vi.restoreAllMocks();
		await rm(testDir, { recursive: true, force: true });
	});

	test("deployment blocked when security violations found in extension", async () => {
		const originalCwd = process.cwd();
		process.chdir(testDir);

		try {
			// Create extension directory with source file
			const ext1Dir = join(extensionsDir, "extension-1");
			await mkdir(ext1Dir, { recursive: true });

			// Create a file with security violation
			await writeFile(
				join(ext1Dir, "index.ts"),
				`
import { exec } from 'child_process';
exec('rm -rf /'); // SEC001 violation
`,
			);

			// Mock config to return the extension
			// Note: source is relative to extensions/{handle}/ directory
			const extensions: ConfigExtensionInfo[] = [
				{
					type: "embed",
					name: "@test/extension-1",
					handle: "extension-1",
					source: "index.ts",
					targets: [{ target: "body.start" }],
				},
			];
			getExtensionsFromConfigSpy.mockReturnValue(extensions);

			const result = await applicationDeploy("test-app");

			expect(result.success).toBe(false);
			expect(result.error?.userMessage).toContain("blocked");
			expect(applicationsService.updateApplication).not.toHaveBeenCalled();
		} finally {
			process.chdir(originalCwd);
		}
	});

	test("deployment succeeds when all extensions are clean", async () => {
		const originalCwd = process.cwd();
		process.chdir(testDir);

		try {
			// Create two clean extensions
			const ext1Dir = join(extensionsDir, "extension-1");
			const ext2Dir = join(extensionsDir, "extension-2");
			await mkdir(ext1Dir, { recursive: true });
			await mkdir(ext2Dir, { recursive: true });

			await writeFile(
				join(ext1Dir, "index.ts"),
				`
export function hello() {
  return "Hello from extension 1!";
}
`,
			);

			await writeFile(
				join(ext2Dir, "index.ts"),
				`
export function greet(name: string) {
  return \`Hello, \${name}!\`;
}
`,
			);

			// Mock config to return the extensions
			// Note: source is relative to extensions/{handle}/ directory
			const extensions: ConfigExtensionInfo[] = [
				{
					type: "embed",
					name: "@test/extension-1",
					handle: "extension-1",
					source: "index.ts",
					targets: [{ target: "body.start" }],
				},
				{
					type: "embed",
					name: "@test/extension-2",
					handle: "extension-2",
					source: "index.ts",
					targets: [{ target: "body.end" }],
				},
			];
			getExtensionsFromConfigSpy.mockReturnValue(extensions);

			const result = await applicationDeploy("test-app");

			expect(result.success).toBe(true);
			expect(result.data?.totalExtensions).toBe(2);
			expect(result.data?.blockedExtensions).toBe(0);
			expect(result.data?.securityReports).toHaveLength(2);
			expect(applicationsService.updateApplication).toHaveBeenCalledWith(
				"app-123",
				{ status: "ACTIVE" },
				{ accessToken: "test-token" },
			);
		} finally {
			process.chdir(originalCwd);
		}
	});

	test("deployment scans all extensions consistently", async () => {
		const originalCwd = process.cwd();
		process.chdir(testDir);

		try {
			// Create three extensions
			const extensions: ConfigExtensionInfo[] = [];
			for (let i = 1; i <= 3; i++) {
				const extDir = join(extensionsDir, `extension-${i}`);
				await mkdir(extDir, { recursive: true });

				await writeFile(
					join(extDir, "index.ts"),
					`
export function func${i}() {
  return ${i};
}
`,
				);

				extensions.push({
					type: "embed",
					name: `@test/extension-${i}`,
					handle: `extension-${i}`,
					source: "index.ts",
					targets: [{ target: "body.start" }],
				});
			}

			// Mock config to return the extensions
			getExtensionsFromConfigSpy.mockReturnValue(extensions);

			const result = await applicationDeploy("test-app");

			expect(result.success).toBe(true);
			expect(result.data?.totalExtensions).toBe(3);
			expect(result.data?.securityReports).toHaveLength(3);

			// Verify all extensions were scanned
			const extensionNames = result.data?.securityReports.map(
				(r) => r.extensionName,
			);
			expect(extensionNames).toContain("@test/extension-1");
			expect(extensionNames).toContain("@test/extension-2");
			expect(extensionNames).toContain("@test/extension-3");
		} finally {
			process.chdir(originalCwd);
		}
	});

	test("deployment blocked if any extension has violations", async () => {
		const originalCwd = process.cwd();
		process.chdir(testDir);

		try {
			// Create two extensions, one clean, one with violation
			const ext1Dir = join(extensionsDir, "extension-clean");
			const ext2Dir = join(extensionsDir, "extension-bad");
			await mkdir(ext1Dir, { recursive: true });
			await mkdir(ext2Dir, { recursive: true });

			await writeFile(
				join(ext1Dir, "index.ts"),
				`
export function safe() {
  return true;
}
`,
			);

			await writeFile(
				join(ext2Dir, "index.ts"),
				`
import { exec } from 'child_process';
exec('dangerous command'); // SEC001
`,
			);

			// Mock config to return the extensions
			// Note: source is relative to extensions/{handle}/ directory
			const extensions: ConfigExtensionInfo[] = [
				{
					type: "embed",
					name: "@test/extension-clean",
					handle: "extension-clean",
					source: "index.ts",
					targets: [{ target: "body.start" }],
				},
				{
					type: "embed",
					name: "@test/extension-bad",
					handle: "extension-bad",
					source: "index.ts",
					targets: [{ target: "body.end" }],
				},
			];
			getExtensionsFromConfigSpy.mockReturnValue(extensions);

			const result = await applicationDeploy("test-app");

			// Deployment should be blocked
			expect(result.success).toBe(false);
			expect(result.error?.userMessage).toContain("blocked");
			expect(applicationsService.updateApplication).not.toHaveBeenCalled();
		} finally {
			process.chdir(originalCwd);
		}
	});

	test("deployment succeeds (no-op) when no extensions found", async () => {
		const originalCwd = process.cwd();
		process.chdir(testDir);

		try {
			// Mock config to return no extensions
			getExtensionsFromConfigSpy.mockReturnValue([]);

			const result = await applicationDeploy("test-app");

			expect(result.success).toBe(true);
			expect(result.data?.totalExtensions).toBe(0);
			expect(result.data?.securityReports).toHaveLength(0);
			// Deployment should still proceed
			expect(applicationsService.updateApplication).toHaveBeenCalledWith(
				"app-123",
				{ status: "ACTIVE" },
				{ accessToken: "test-token" },
			);
		} finally {
			process.chdir(originalCwd);
		}
	});
});
