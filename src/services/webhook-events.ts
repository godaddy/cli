import { Fetch } from "@effect/platform/FetchHttpClient";
import type { FileSystem } from "@effect/platform/FileSystem";
import * as Effect from "effect/Effect";
import { type Environment, envGetEffect, getApiUrl } from "../core/environment";
import {
  AuthenticationError,
  type ConfigurationError,
  NetworkError,
  type ValidationError,
} from "../effect/errors";
import { cliTraceHeaders } from "../shared/cli-trace";
import { logHttpRequest, logHttpResponse } from "./logger";

export type WebhookEventType = {
  eventType: string;
  description: string;
};

/**
 * Fetch webhook event types from the API.
 * Requires FileSystem service (via envGetEffect) and HttpClient.
 */
export function getWebhookEventsTypesEffect({
  accessToken,
}: {
  accessToken: string | null;
}): Effect.Effect<
  { events: Array<WebhookEventType> },
  AuthenticationError | NetworkError | ConfigurationError | ValidationError,
  FileSystem | Fetch
> {
  return Effect.gen(function* () {
    if (!accessToken) {
      return yield* Effect.fail(
        new AuthenticationError({
          message: "Access token is required",
          userMessage: "Authentication required to fetch webhook events",
        }),
      );
    }

    const fetch = yield* Fetch;

    // Get the current environment and build the API URL
    const env: Environment = yield* envGetEffect();
    const baseUrl = getApiUrl(env);

    const url = `${baseUrl}/v1/apis/webhook-event-types`;
    const headers = {
      Authorization: `Bearer ${accessToken}`,
      ...cliTraceHeaders(),
    };

    const startTime = Date.now();

    // Log HTTP request
    logHttpRequest({
      method: "GET",
    });

    const response = yield* Effect.tryPromise({
      try: () => fetch(url, { headers }),
      catch: (error) =>
        new NetworkError({
          message: `Failed to fetch webhook events: ${error instanceof Error ? error.message : String(error)}`,
          userMessage: "Network error fetching webhook events",
        }),
    });

    const duration = Date.now() - startTime;

    const json = yield* Effect.tryPromise({
      try: () =>
        response.json() as Promise<{
          events: Array<WebhookEventType>;
          error?: string;
        }>,
      catch: (error) =>
        new NetworkError({
          message: `Failed to parse webhook events response: ${error instanceof Error ? error.message : String(error)}`,
          userMessage: "Failed to parse webhook events response",
        }),
    });

    // Log HTTP response
    logHttpResponse({
      method: "GET",
      status: response.status,
      statusText: response.statusText,
      headers: response.headers ? {} : undefined,
      body: json,
      duration,
    });

    if (!response.ok) {
      return yield* Effect.fail(
        new AuthenticationError({
          message: json.error || "Authentication failed",
          userMessage:
            response.status === 401 || response.status === 403
              ? "Authentication failed. Run 'godaddy auth login'."
              : `Webhook events request failed with status ${response.status}`,
        }),
      );
    }

    return json as { events: Array<WebhookEventType> };
  });
}
