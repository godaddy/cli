/**
 * Presigned URL service for extension artifact uploads
 * Phase 3: GraphQL integration for generateReleaseUploadUrl mutation
 */

import { type CliError, NetworkError } from "@/effect/errors";
import { mapGraphQLError } from "@/services/graphql-error";
import {
  getRequestHeaders,
  makeGraphQLClientEffect,
} from "@/services/http-helpers";
import { getLogger } from "@/services/logger";
import type { Fetch } from "@effect/platform/FetchHttpClient";
import type { FileSystem } from "@effect/platform/FileSystem";
import * as Effect from "effect/Effect";
import { graphql } from "gql.tada";

const logger = getLogger();

/**
 * Upload target information from GraphQL
 */
export interface UploadTarget {
  uploadId: string;
  url: string;
  key: string;
  expiresAt: string;
  maxSizeBytes: number;
  requiredHeaders: Record<string, string>;
}

/**
 * Parameters for requesting an upload URL
 */
export interface GetUploadTargetParams {
  applicationId: string;
  releaseId: string;
  contentType?: "JS" | "ZIP" | "TAR";
  /** Target location for the extension (e.g., "body.end") - becomes filename {target}.js */
  target?: string;
}

const GenerateReleaseUploadUrlMutation = graphql(`
  mutation GenerateReleaseUploadUrl($input: MutationGenerateReleaseUploadUrlInput!) {
    generateReleaseUploadUrl(input: $input) {
      uploadId
      url
      key
      expiresAt
      maxSizeBytes
      requiredHeaders
    }
  }
`);

/**
 * Get a presigned upload URL for an extension artifact.
 * Requires FileSystem service (via initApiBaseUrlEffect).
 */
export function getUploadTargetEffect(
  params: GetUploadTargetParams,
  accessToken: string,
): Effect.Effect<UploadTarget, CliError, FileSystem | Fetch> {
  return Effect.gen(function* () {
    logger.debug(
      {
        applicationId: params.applicationId,
        releaseId: params.releaseId,
        contentType: params.contentType ?? "JS",
      },
      "Requesting presigned upload URL",
    );

    const client = yield* makeGraphQLClientEffect();

    // Use the shared mapGraphQLError so presigned-URL failures produce
    // the same tagged-error classification and envelope shape as the rest
    // of the source-of-truth calls (ServerError with a `kind` discriminant
    // for classified server errors, AuthenticationError for auth failures,
    // NetworkError with HTTP context for unknown/transport failures).
    const response = yield* Effect.tryPromise({
      try: () =>
        client.request(
          GenerateReleaseUploadUrlMutation,
          {
            input: {
              applicationId: params.applicationId,
              releaseId: params.releaseId,
              contentType: params.contentType ?? "JS",
              target: params.target,
            },
          },
          getRequestHeaders(accessToken),
        ),
      catch: mapGraphQLError,
    });

    if (!response.generateReleaseUploadUrl) {
      return yield* Effect.fail(
        new NetworkError({
          message: "Failed to generate upload URL: empty response",
          userMessage: "Failed to generate presigned upload URL",
        }),
      );
    }

    const data = response.generateReleaseUploadUrl;

    // Parse requiredHeaders from array of "key:value" strings to Record
    const headersMap: Record<string, string> = {};
    for (const header of data.requiredHeaders) {
      const [key, ...valueParts] = header.split(":");
      if (key && valueParts.length > 0) {
        headersMap[key.trim()] = valueParts.join(":").trim();
      }
    }

    logger.debug(
      {
        uploadId: data.uploadId,
        key: data.key,
        expiresAt: data.expiresAt,
        maxSizeBytes: data.maxSizeBytes,
      },
      "Received presigned upload URL",
    );

    return {
      uploadId: data.uploadId,
      url: data.url,
      key: data.key,
      expiresAt: data.expiresAt,
      maxSizeBytes: data.maxSizeBytes,
      requiredHeaders: headersMap,
    };
  });
}
