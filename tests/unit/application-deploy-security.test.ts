import { existsSync } from "node:fs";
import { mkdir, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import * as Effect from "effect/Effect";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import {
  type DeployProgressEvent,
  applicationDeployEffect,
} from "../../src/core/applications";
import * as authModule from "../../src/core/auth";
import { NetworkError } from "../../src/effect/errors";
import * as applicationsService from "../../src/services/applications";
import * as configService from "../../src/services/config";
import type { ConfigExtensionInfo } from "../../src/services/config";
import * as bundlerService from "../../src/services/extension/bundler";
import * as presignedUrlService from "../../src/services/extension/presigned-url";
import * as securityScanService from "../../src/services/extension/security-scan";
import * as uploadService from "../../src/services/extension/upload";
import {
  extractFailure,
  runEffect,
  runEffectExit,
} from "../setup/effect-test-utils";

const testDir = join(process.cwd(), "tests", "fixtures", "test-app-deploy");
const extensionsDir = join(testDir, "extensions");

describe("Application Deploy with Security Scanning", () => {
  let getExtensionsFromConfigSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(async () => {
    // Clean up and create test directory with extensions folder
    await rm(testDir, { recursive: true, force: true });
    await mkdir(extensionsDir, { recursive: true });

    // Mock authentication
    vi.spyOn(authModule, "getFromKeychainEffect").mockReturnValue(
      Effect.succeed("test-token" as string | null),
    );

    // Mock getApplicationAndLatestReleaseEffect to return a valid application with release
    vi.spyOn(
      applicationsService,
      "getApplicationAndLatestReleaseEffect",
    ).mockReturnValue(
      Effect.succeed({
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
      }),
    );

    // Mock updateApplicationEffect
    vi.spyOn(applicationsService, "updateApplicationEffect").mockReturnValue(
      Effect.succeed({
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
      }),
    );

    vi.spyOn(applicationsService, "activateReleaseEffect").mockReturnValue(
      Effect.succeed({
        activateRelease: {
          id: "release-123",
          version: "1.0.0",
          description: "Initial release",
          status: "ACTIVE",
          activatedAt: "2025-01-01T00:00:00Z",
          createdAt: "2025-01-01T00:00:00Z",
          updatedAt: "2025-01-01T00:00:00Z",
        },
      }),
    );

    // Setup spy for getExtensionsFromConfig (will be configured per test)
    getExtensionsFromConfigSpy = vi.spyOn(
      configService,
      "getExtensionsFromConfigEffect",
    );

    // Mock presigned URL generation
    vi.spyOn(presignedUrlService, "getUploadTargetEffect").mockReturnValue(
      Effect.succeed({
        uploadId: "upload-123",
        url: "https://s3.example.com/presigned-url",
        key: "test/bundle.js",
        expiresAt: "2025-12-31T23:59:59Z",
        maxSizeBytes: 10485760,
        requiredHeaders: {},
      }),
    );

    // Mock S3 upload
    vi.spyOn(uploadService, "uploadArtifactEffect").mockReturnValue(
      Effect.succeed({
        uploadId: "upload-123",
        etag: '"abc123"',
        status: 200,
        sizeBytes: 1000,
      }),
    );
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
      getExtensionsFromConfigSpy.mockReturnValue(Effect.succeed(extensions));

      const exit = await runEffectExit(applicationDeployEffect("test-app"));
      const err = extractFailure(exit) as { userMessage: string };
      expect(err.userMessage).toContain("blocked");
      expect(applicationsService.activateReleaseEffect).not.toHaveBeenCalled();
      expect(
        applicationsService.updateApplicationEffect,
      ).not.toHaveBeenCalled();
    } finally {
      process.chdir(originalCwd);
    }
  });

  test("deployment succeeds when all extensions are clean", async () => {
    const originalCwd = process.cwd();
    process.chdir(testDir);

    try {
      const progressEvents: DeployProgressEvent[] = [];

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
      getExtensionsFromConfigSpy.mockReturnValue(Effect.succeed(extensions));

      const result = await runEffect(
        applicationDeployEffect("test-app", {
          onProgress: (event) => {
            progressEvents.push(event);
          },
        }),
      );

      expect(result.totalExtensions).toBe(2);
      expect(result.blockedExtensions).toBe(0);
      expect(result.securityReports).toHaveLength(2);
      expect(result.securityReports[0].preBundleReport).toBeDefined();
      expect(result.securityReports[0].postBundleReport).toBeDefined();
      expect(result.securityReports[0].postBundleReport?.blocked).toBe(false);
      expect(
        progressEvents.some(
          (event) =>
            event.type === "step" &&
            event.name === "scan.prebundle" &&
            event.status === "started",
        ),
      ).toBe(true);
      expect(
        progressEvents.some(
          (event) =>
            event.type === "step" &&
            event.name === "scan.postbundle" &&
            event.status === "completed",
        ),
      ).toBe(true);
      expect(
        progressEvents.some(
          (event) =>
            event.type === "step" &&
            event.name === "release.activate" &&
            event.status === "completed",
        ),
      ).toBe(true);
      expect(
        progressEvents.some(
          (event) =>
            event.type === "step" &&
            event.name === "deploy" &&
            event.status === "completed",
        ),
      ).toBe(true);
      expect(applicationsService.activateReleaseEffect).toHaveBeenCalledWith(
        "app-123",
        "release-123",
        { accessToken: "test-token" },
      );
      expect(applicationsService.updateApplicationEffect).toHaveBeenCalledWith(
        "app-123",
        { status: "ACTIVE" },
        { accessToken: "test-token" },
      );
      const activateOrder =
        vi.mocked(applicationsService.activateReleaseEffect).mock
          .invocationCallOrder[0];
      const updateOrder =
        vi.mocked(applicationsService.updateApplicationEffect).mock
          .invocationCallOrder[0];
      expect(activateOrder).toBeLessThan(updateOrder);
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
      getExtensionsFromConfigSpy.mockReturnValue(Effect.succeed(extensions));

      const result = await runEffect(applicationDeployEffect("test-app"));

      expect(result.totalExtensions).toBe(3);
      expect(result.securityReports).toHaveLength(3);

      // Verify all extensions were scanned
      const extensionNames = result.securityReports.map((r) => r.extensionName);
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
      getExtensionsFromConfigSpy.mockReturnValue(Effect.succeed(extensions));

      // Deployment should be blocked
      const exit = await runEffectExit(applicationDeployEffect("test-app"));
      const err = extractFailure(exit) as { userMessage: string };
      expect(err.userMessage).toContain("blocked");
      expect(applicationsService.activateReleaseEffect).not.toHaveBeenCalled();
      expect(
        applicationsService.updateApplicationEffect,
      ).not.toHaveBeenCalled();
    } finally {
      process.chdir(originalCwd);
    }
  });

  test("deployment succeeds (no-op) when no extensions found", async () => {
    const originalCwd = process.cwd();
    process.chdir(testDir);

    try {
      // Mock config to return no extensions
      getExtensionsFromConfigSpy.mockReturnValue(Effect.succeed([]));

      const result = await runEffect(applicationDeployEffect("test-app"));

      expect(result.totalExtensions).toBe(0);
      expect(result.securityReports).toHaveLength(0);
      // Deployment should still proceed
      expect(applicationsService.activateReleaseEffect).toHaveBeenCalledWith(
        "app-123",
        "release-123",
        { accessToken: "test-token" },
      );
      expect(applicationsService.updateApplicationEffect).toHaveBeenCalledWith(
        "app-123",
        { status: "ACTIVE" },
        { accessToken: "test-token" },
      );
      const activateOrder =
        vi.mocked(applicationsService.activateReleaseEffect).mock
          .invocationCallOrder[0];
      const updateOrder =
        vi.mocked(applicationsService.updateApplicationEffect).mock
          .invocationCallOrder[0];
      expect(activateOrder).toBeLessThan(updateOrder);
    } finally {
      process.chdir(originalCwd);
    }
  });

  test("deployment rejects extension handle traversal outside ./extensions", async () => {
    const originalCwd = process.cwd();
    process.chdir(testDir);

    try {
      getExtensionsFromConfigSpy.mockReturnValue(
        Effect.succeed([
          {
            type: "embed",
            name: "@test/bad-handle",
            handle: "../outside",
            source: "index.ts",
            targets: [{ target: "body.start" }],
          },
        ]),
      );

      const exit = await runEffectExit(applicationDeployEffect("test-app"));
      const err = extractFailure(exit) as { userMessage: string };
      expect(err.userMessage).toContain("handle path");
      expect(applicationsService.activateReleaseEffect).not.toHaveBeenCalled();
      expect(
        applicationsService.updateApplicationEffect,
      ).not.toHaveBeenCalled();
    } finally {
      process.chdir(originalCwd);
    }
  });

  test("deployment rejects extension source traversal outside extension directory", async () => {
    const originalCwd = process.cwd();
    process.chdir(testDir);

    try {
      getExtensionsFromConfigSpy.mockReturnValue(
        Effect.succeed([
          {
            type: "embed",
            name: "@test/bad-source",
            handle: "bad-source",
            source: "../outside.ts",
            targets: [{ target: "body.start" }],
          },
        ]),
      );

      const exit = await runEffectExit(applicationDeployEffect("test-app"));
      const err = extractFailure(exit) as { userMessage: string };
      expect(err.userMessage).toContain("source path");
      expect(applicationsService.activateReleaseEffect).not.toHaveBeenCalled();
      expect(
        applicationsService.updateApplicationEffect,
      ).not.toHaveBeenCalled();
    } finally {
      process.chdir(originalCwd);
    }
  });

  test("deployment cleans bundle artifacts when upload fails", async () => {
    const originalCwd = process.cwd();
    process.chdir(testDir);

    try {
      const extDir = join(extensionsDir, "extension-upload-fail");
      await mkdir(extDir, { recursive: true });
      await writeFile(
        join(extDir, "index.ts"),
        `
export function run() {
  return "ok";
}
`,
      );

      getExtensionsFromConfigSpy.mockReturnValue(
        Effect.succeed([
          {
            type: "embed",
            name: "@test/extension-upload-fail",
            handle: "extension-upload-fail",
            source: "index.ts",
            targets: [{ target: "body.start" }],
          },
        ]),
      );

      const artifactPath = join(testDir, "upload-fail.bundle.mjs");
      const sourcemapPath = `${artifactPath}.map`;
      await writeFile(artifactPath, "export const value = 1;");
      await writeFile(sourcemapPath, "{}");

      const cleanReport = {
        findings: [],
        blocked: false,
        summary: {
          total: 0,
          byRuleId: {},
          bySeverity: { block: 0, warn: 0, off: 0 },
        },
        scannedFiles: 1,
      };

      vi.spyOn(securityScanService, "scanExtensionEffect").mockReturnValue(
        Effect.succeed(cleanReport),
      );
      vi.spyOn(bundlerService, "bundleExtensionEffect").mockReturnValue(
        Effect.succeed({
          packageName: "extension-upload-fail",
          artifactName: "upload-fail.bundle.mjs",
          artifactPath,
          size: 24,
          sha256: "deadbeef",
          sourcemapPath,
        }),
      );
      vi.spyOn(securityScanService, "scanBundleEffect").mockReturnValue(
        Effect.succeed(cleanReport),
      );
      vi.spyOn(uploadService, "uploadArtifactEffect").mockReturnValue(
        Effect.fail(
          new NetworkError({
            message: "upload failed",
            userMessage: "upload failed",
          }),
        ),
      );

      const exit = await runEffectExit(applicationDeployEffect("test-app"));
      const err = extractFailure(exit) as { userMessage: string };
      expect(err.userMessage).toBe("upload failed");
      expect(existsSync(artifactPath)).toBe(false);
      expect(existsSync(sourcemapPath)).toBe(false);
      expect(applicationsService.activateReleaseEffect).not.toHaveBeenCalled();
      expect(
        applicationsService.updateApplicationEffect,
      ).not.toHaveBeenCalled();
    } finally {
      process.chdir(originalCwd);
    }
  });
});
