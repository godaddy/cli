/**
 * S3 upload service for extension artifacts
 * Phase 4: HTTP PUT to presigned URLs with retry logic
 */

import { promises as fs } from "node:fs";
import { getLogger } from "@/services/logger";
import type { UploadTarget } from "./presigned-url";

const logger = getLogger();

/**
 * Upload result metadata
 */
export interface UploadResult {
	uploadId: string;
	etag?: string;
	status: number;
	sizeBytes: number;
}

/**
 * Upload options
 */
export interface UploadOptions {
	/**
	 * Number of retry attempts for transient errors (default: 3)
	 */
	maxRetries?: number;
	/**
	 * Base delay in ms for exponential backoff (default: 250)
	 */
	baseDelayMs?: number;
	/**
	 * Content type override (default: application/javascript)
	 */
	contentType?: string;
}

/**
 * Upload an artifact to S3 using presigned URL
 *
 * Implements retry logic with exponential backoff for transient errors (5xx, network).
 * Retries: 250ms, 750ms, 1500ms (by default)
 */
export async function uploadArtifact(
	target: UploadTarget,
	filePath: string,
	options: UploadOptions = {},
): Promise<UploadResult> {
	const maxRetries = options.maxRetries ?? 3;
	const baseDelay = options.baseDelayMs ?? 250;
	const contentType = options.contentType ?? "application/javascript";

	const fileBuffer = await fs.readFile(filePath);
	const sizeBytes = fileBuffer.byteLength;

	// Validate file size
	if (sizeBytes > target.maxSizeBytes) {
		throw new Error(
			`File size (${sizeBytes} bytes) exceeds maximum allowed (${target.maxSizeBytes} bytes)`,
		);
	}

	logger.debug(
		{
			uploadId: target.uploadId,
			sizeBytes,
			maxSizeBytes: target.maxSizeBytes,
			contentType,
		},
		"Starting artifact upload",
	);

	let lastError: Error | undefined;

	for (let attempt = 1; attempt <= maxRetries; attempt++) {
		try {
			// Use only the requiredHeaders from the presigned URL (already signed)
			// Filter out x-amz-meta-upload-id as it's not signed
			const { "x-amz-meta-upload-id": _, ...headers } = target.requiredHeaders;

			logger.debug(
				{
					uploadId: target.uploadId,
					attempt,
					maxRetries,
					headers: Object.keys(headers),
				},
				"Attempting upload",
			);

			const response = await fetch(target.url, {
				method: "PUT",
				headers,
				body: fileBuffer,
			});

			if (response.ok) {
				const etag = response.headers.get("etag") ?? undefined;

				logger.info(
					{
						uploadId: target.uploadId,
						key: target.key,
						status: response.status,
						etag,
						sizeBytes,
						attempt,
					},
					"Upload successful",
				);

				return {
					uploadId: target.uploadId,
					etag,
					status: response.status,
					sizeBytes,
				};
			}

			// Read response body for error details
			const responseText = await response.text().catch(() => "");
			const errorSnippet = responseText.slice(0, 200);

			// Retry on server errors (5xx)
			if (response.status >= 500 && response.status < 600) {
				lastError = new Error(
					`Upload failed with status ${response.status}: ${errorSnippet}`,
				);

				logger.warn(
					{
						uploadId: target.uploadId,
						status: response.status,
						attempt,
						maxRetries,
						errorSnippet,
					},
					"Upload failed with server error, retrying",
				);

				// Exponential backoff: 250ms, 750ms, 1500ms
				if (attempt < maxRetries) {
					const delay = baseDelay * 3 ** (attempt - 1);
					await new Promise((resolve) => setTimeout(resolve, delay));
				}
			} else {
				// Client errors (4xx) are not retryable
				throw new Error(
					`Upload failed with status ${response.status}: ${errorSnippet}`,
				);
			}
		} catch (err) {
			// Network errors are retryable
			if (
				err instanceof TypeError &&
				(err.message.includes("fetch") || err.message.includes("network"))
			) {
				lastError = err as Error;

				logger.warn(
					{
						uploadId: target.uploadId,
						attempt,
						maxRetries,
						error: err.message,
					},
					"Upload failed with network error, retrying",
				);

				if (attempt < maxRetries) {
					const delay = baseDelay * 3 ** (attempt - 1);
					await new Promise((resolve) => setTimeout(resolve, delay));
				}
			} else {
				// Re-throw non-retryable errors
				throw err;
			}
		}
	}

	// All retries exhausted
	throw new Error(
		`Upload failed after ${maxRetries} attempts: ${lastError?.message ?? "unknown error"}`,
	);
}
