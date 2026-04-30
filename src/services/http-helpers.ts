/**
 * Shared HTTP helpers for API requests
 */

import { Fetch } from "@effect/platform/FetchHttpClient";
import type { FileSystem } from "@effect/platform/FileSystem";
import * as Effect from "effect/Effect";
import { GraphQLClient } from "graphql-request";
import { v7 as uuid } from "uuid";
import packageJson from "../../package.json";
import { type Environment, envGetEffect, getApiUrl } from "../core/environment";
import { ConfigurationError } from "../effect/errors";

/** Shared User-Agent for outbound CLI HTTP requests (server-side tracing). */
export const CLI_USER_AGENT = `godaddy-cli/${packageJson.version}`;

/**
 * Headers attached to outbound requests so upstream logs can correlate calls.
 * Each invocation generates a fresh request ID.
 */
export function cliTraceHeaders(): Record<string, string> {
  return {
    "User-Agent": CLI_USER_AGENT,
    "X-Request-ID": uuid(),
  };
}

/**
 * Resolve the API base URL from environment variables or the active environment.
 * Pure function — no caching.
 */
export function initApiBaseUrlEffect(): Effect.Effect<
  string,
  ConfigurationError,
  FileSystem
> {
  return Effect.gen(function* () {
    if (process.env.APPLICATIONS_GRAPHQL_URL) {
      return process.env.APPLICATIONS_GRAPHQL_URL;
    }

    const env: Environment = yield* envGetEffect();
    return `${getApiUrl(env)}/v1/apps/app-registry-subgraph`;
  }).pipe(
    Effect.catchAll((error) =>
      Effect.fail(
        "_tag" in error && error._tag === "ConfigurationError"
          ? (error as ConfigurationError)
          : new ConfigurationError({
              message: `Failed to initialize API base URL: ${error}`,
              userMessage: "Could not determine API base URL",
            }),
      ),
    ),
  );
}

/**
 * Create a GraphQLClient wired to the injectable Fetch service.
 * This ensures all GraphQL requests go through the same fetch implementation
 * that the rest of the codebase uses, making them interceptable in tests.
 */
export function makeGraphQLClientEffect(): Effect.Effect<
  GraphQLClient,
  ConfigurationError,
  FileSystem | Fetch
> {
  return Effect.gen(function* () {
    const baseUrl = yield* initApiBaseUrlEffect();
    const fetch = yield* Fetch;
    return new GraphQLClient(baseUrl, { fetch });
  });
}

/**
 * Get standard request headers with authentication
 */
export function getRequestHeaders(accessToken: string): Record<string, string> {
  return {
    Authorization: `Bearer ${accessToken}`,
    ...cliTraceHeaders(),
  };
}
