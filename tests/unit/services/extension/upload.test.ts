import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { UploadTarget } from "@/services/extension/presigned-url";
import { uploadArtifact } from "@/services/extension/upload";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Mock logger to avoid undefined errors in tests
vi.mock("@/services/logger", () => ({
	getLogger: () => ({
		debug: vi.fn(),
		info: vi.fn(),
		warn: vi.fn(),
		error: vi.fn(),
	}),
}));

describe("upload service", () => {
	let testDir: string;

	beforeEach(async () => {
		testDir = await mkdtemp(join(tmpdir(), "upload-test-"));
	});

	afterEach(async () => {
		await rm(testDir, { recursive: true, force: true });
		vi.restoreAllMocks();
	});

	describe("uploadArtifact", () => {
		it("should upload artifact successfully", async () => {
			const artifactPath = join(testDir, "test.js");
			const artifactContent = "console.log('test');";
			await writeFile(artifactPath, artifactContent);

			const mockResponse = new Response(null, {
				status: 200,
				headers: {
					etag: '"abc123"',
				},
			});

			global.fetch = vi.fn().mockResolvedValue(mockResponse);

			const target: UploadTarget = {
				uploadId: "upload-123",
				url: "https://s3.example.com/presigned-url",
				key: "test/test.js",
				expiresAt: "2025-11-14T16:00:00Z",
				maxSizeBytes: 10485760,
				requiredHeaders: {
					"x-amz-meta-upload-id": "upload-123",
					"content-type": "application/javascript",
				},
			};

			const result = await uploadArtifact(target, artifactPath);

			expect(result).toEqual({
				uploadId: "upload-123",
				etag: '"abc123"',
				status: 200,
				sizeBytes: Buffer.byteLength(artifactContent),
			});

			// Implementation uses only requiredHeaders, filtering out x-amz-meta-upload-id
			expect(fetch).toHaveBeenCalledWith(
				target.url,
				expect.objectContaining({
					method: "PUT",
					headers: {
						"content-type": "application/javascript",
					},
				}),
			);
		});

		it("should reject file larger than maxSizeBytes", async () => {
			const artifactPath = join(testDir, "large.js");
			await writeFile(artifactPath, "x".repeat(1000));

			const target: UploadTarget = {
				uploadId: "upload-123",
				url: "https://s3.example.com/presigned-url",
				key: "test/large.js",
				expiresAt: "2025-11-14T16:00:00Z",
				maxSizeBytes: 100, // Smaller than file
				requiredHeaders: {},
			};

			await expect(uploadArtifact(target, artifactPath)).rejects.toThrow(
				"File size (1000 bytes) exceeds maximum allowed (100 bytes)",
			);
		});

		it("should retry on 5xx server errors", async () => {
			const artifactPath = join(testDir, "test.js");
			await writeFile(artifactPath, "console.log('test');");

			const mockFailResponse = new Response("Server error", { status: 503 });
			const mockSuccessResponse = new Response(null, {
				status: 200,
				headers: { etag: '"retry-success"' },
			});

			global.fetch = vi
				.fn()
				.mockResolvedValueOnce(mockFailResponse) // First attempt fails
				.mockResolvedValueOnce(mockSuccessResponse); // Second attempt succeeds

			const target: UploadTarget = {
				uploadId: "upload-123",
				url: "https://s3.example.com/presigned-url",
				key: "test/test.js",
				expiresAt: "2025-11-14T16:00:00Z",
				maxSizeBytes: 10485760,
				requiredHeaders: {},
			};

			const result = await uploadArtifact(target, artifactPath, {
				baseDelayMs: 10, // Faster for testing
			});

			expect(result.etag).toBe('"retry-success"');
			expect(fetch).toHaveBeenCalledTimes(2);
		});

		it("should fail after exhausting retries on 5xx errors", async () => {
			const artifactPath = join(testDir, "test.js");
			await writeFile(artifactPath, "console.log('test');");

			const mockFailResponse = new Response("Server error", { status: 500 });
			global.fetch = vi.fn().mockResolvedValue(mockFailResponse);

			const target: UploadTarget = {
				uploadId: "upload-123",
				url: "https://s3.example.com/presigned-url",
				key: "test/test.js",
				expiresAt: "2025-11-14T16:00:00Z",
				maxSizeBytes: 10485760,
				requiredHeaders: {},
			};

			await expect(
				uploadArtifact(target, artifactPath, {
					maxRetries: 3,
					baseDelayMs: 10,
				}),
			).rejects.toThrow("Upload failed after 3 attempts");

			expect(fetch).toHaveBeenCalledTimes(3);
		});

		it("should not retry on 4xx client errors", async () => {
			const artifactPath = join(testDir, "test.js");
			await writeFile(artifactPath, "console.log('test');");

			const mockFailResponse = new Response("Forbidden", { status: 403 });
			global.fetch = vi.fn().mockResolvedValue(mockFailResponse);

			const target: UploadTarget = {
				uploadId: "upload-123",
				url: "https://s3.example.com/presigned-url",
				key: "test/test.js",
				expiresAt: "2025-11-14T16:00:00Z",
				maxSizeBytes: 10485760,
				requiredHeaders: {},
			};

			await expect(uploadArtifact(target, artifactPath)).rejects.toThrow(
				"Upload failed with status 403",
			);

			expect(fetch).toHaveBeenCalledTimes(1); // No retries
		});

		it("should use content type from requiredHeaders", async () => {
			const artifactPath = join(testDir, "test.zip");
			await writeFile(artifactPath, "fake-zip-content");

			const mockResponse = new Response(null, { status: 200 });
			global.fetch = vi.fn().mockResolvedValue(mockResponse);

			const target: UploadTarget = {
				uploadId: "upload-123",
				url: "https://s3.example.com/presigned-url",
				key: "test/test.zip",
				expiresAt: "2025-11-14T16:00:00Z",
				maxSizeBytes: 10485760,
				requiredHeaders: {
					"content-type": "application/zip",
				},
			};

			await uploadArtifact(target, artifactPath);

			expect(fetch).toHaveBeenCalledWith(
				target.url,
				expect.objectContaining({
					headers: {
						"content-type": "application/zip",
					},
				}),
			);
		});

		it("should handle network errors with retry", async () => {
			const artifactPath = join(testDir, "test.js");
			await writeFile(artifactPath, "console.log('test');");

			const networkError = new TypeError("fetch failed");
			const mockSuccessResponse = new Response(null, { status: 200 });

			global.fetch = vi
				.fn()
				.mockRejectedValueOnce(networkError) // Network error
				.mockResolvedValueOnce(mockSuccessResponse); // Success

			const target: UploadTarget = {
				uploadId: "upload-123",
				url: "https://s3.example.com/presigned-url",
				key: "test/test.js",
				expiresAt: "2025-11-14T16:00:00Z",
				maxSizeBytes: 10485760,
				requiredHeaders: {},
			};

			const result = await uploadArtifact(target, artifactPath, {
				baseDelayMs: 10,
			});

			expect(result.status).toBe(200);
			expect(fetch).toHaveBeenCalledTimes(2);
		});
	});
});
