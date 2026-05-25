/**
 * S3 upload service for extension artifacts
 * Phase 4: HTTP PUT to presigned URLs with retry logic
 */

import { getLogger } from "@/services/logger";
import { Fetch } from "@effect/platform/FetchHttpClient";
import { FileSystem } from "@effect/platform/FileSystem";
import * as Effect from "effect/Effect";
import { NetworkError } from "../../effect/errors";
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

function sleep(ms: number): Effect.Effect<void, never, never> {
  return Effect.sleep(`${ms} millis`);
}

/**
 * Upload an artifact to S3 using presigned URL.
 *
 * Implements retry logic with exponential backoff for transient errors (5xx, network).
 * Retries: 250ms, 750ms, 1500ms (by default)
 */
export function uploadArtifactEffect(
  target: UploadTarget,
  filePath: string,
  options: UploadOptions = {},
): Effect.Effect<UploadResult, NetworkError, FileSystem | Fetch> {
  return Effect.gen(function* () {
    const fs = yield* FileSystem;
    const fetch = yield* Fetch;
    const maxRetries = options.maxRetries ?? 3;
    const baseDelay = options.baseDelayMs ?? 250;
    const _contentType = options.contentType ?? "application/javascript";

    // Read file content via platform FileSystem service
    const fileContent = yield* fs.readFileString(filePath).pipe(
      Effect.mapError(
        (error) =>
          new NetworkError({
            message: `Failed to read artifact file: ${error.message}`,
            userMessage: "Failed to upload extension artifact",
          }),
      ),
    );

    const fileBuffer = Buffer.from(fileContent);
    const sizeBytes = fileBuffer.byteLength;

    // Validate file size
    if (sizeBytes > target.maxSizeBytes) {
      return yield* Effect.fail(
        new NetworkError({
          message: `File size (${sizeBytes} bytes) exceeds maximum allowed (${target.maxSizeBytes} bytes)`,
          userMessage: "Failed to upload extension artifact",
        }),
      );
    }

    logger.debug(
      {
        uploadId: target.uploadId,
        sizeBytes,
        maxSizeBytes: target.maxSizeBytes,
        contentType: _contentType,
      },
      "Starting artifact upload",
    );

    let lastError: Error | undefined;

    for (let attempt = 1; attempt <= maxRetries; attempt++) {
      // Presigned S3 PUT: send only the headers returned with the URL (they are
      // part of the AWS Signature v4 signing string). Do not add `user-agent` or
      // `x-request-id` here — unsigned headers can still affect behavior, and
      // anything not in the signature can break the upload. Traceability for
      // this hop is the presigned URL request (GraphQL), not the PUT itself.
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

      const fetchResult = yield* Effect.tryPromise({
        try: () =>
          fetch(target.url, {
            method: "PUT",
            headers,
            body: fileBuffer,
          }),
        catch: (err) => err as Error,
      }).pipe(Effect.either);

      if (fetchResult._tag === "Left") {
        const err = fetchResult.left;
        // Network errors are retryable
        if (
          err instanceof TypeError &&
          (err.message.includes("fetch") || err.message.includes("network"))
        ) {
          lastError = err;
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
            yield* sleep(delay);
          }
          continue;
        }
        // Non-retryable error
        return yield* Effect.fail(
          new NetworkError({
            message: err.message,
            userMessage: "Failed to upload extension artifact",
          }),
        );
      }

      const response = fetchResult.right;

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
      const responseText = yield* Effect.tryPromise({
        try: () => response.text(),
        catch: () => new Error(""),
      }).pipe(Effect.orElseSucceed(() => ""));
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

        if (attempt < maxRetries) {
          const delay = baseDelay * 3 ** (attempt - 1);
          yield* sleep(delay);
        }
      } else {
        // Client errors (4xx) are not retryable
        return yield* Effect.fail(
          new NetworkError({
            message: `Upload failed with status ${response.status}: ${errorSnippet}`,
            userMessage: "Failed to upload extension artifact",
          }),
        );
      }
    }

    // All retries exhausted
    return yield* Effect.fail(
      new NetworkError({
        message: `Upload failed after ${maxRetries} attempts: ${lastError?.message ?? "unknown error"}`,
        userMessage: "Failed to upload extension artifact",
      }),
    );
  });
}
