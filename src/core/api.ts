import { Fetch } from "@effect/platform/FetchHttpClient";
import { FileSystem } from "@effect/platform/FileSystem";
import * as Effect from "effect/Effect";
import { v7 as uuid } from "uuid";
import {
  AuthenticationError,
  type CliError,
  NetworkError,
  ValidationError,
} from "../effect/errors";
import { fileExists } from "../effect/fs-utils";
import type { Keychain } from "../effect/services/keychain";
import { CLI_USER_AGENT } from "../shared/cli-trace";
import { getTokenInfoEffect } from "./auth";
import { type Environment, envGetEffect, getApiUrl } from "./environment";

// Minimum seconds before expiry to consider token valid for a request
const TOKEN_EXPIRY_BUFFER_SECONDS = 30;

// Header names (lowercased) that must be redacted from debug output and
// the --include envelope to prevent leaking tokens, cookies, or secrets.
const SENSITIVE_HEADER_PARTS = [
  "authorization",
  "cookie",
  "set-cookie",
  "token",
  "secret",
  "api-key",
  "apikey",
  "x-auth",
] as const;

function isSensitiveHeader(headerName: string): boolean {
  const lower = headerName.toLowerCase();
  return SENSITIVE_HEADER_PARTS.some((part) => lower.includes(part));
}

/**
 * Return a copy of headers with sensitive values replaced by "[REDACTED]".
 */
export { sanitizeHeaders as sanitizeResponseHeaders };

function sanitizeHeaders(
  headers: Record<string, string>,
): Record<string, string> {
  const sanitized: Record<string, string> = {};
  for (const [key, value] of Object.entries(headers)) {
    sanitized[key] = isSensitiveHeader(key) ? "[REDACTED]" : value;
  }
  return sanitized;
}

/**
 * Redact values whose keys look like they contain secrets.
 */
function redactSensitiveBodyFields(body: string): string {
  try {
    const parsed = JSON.parse(body);
    if (typeof parsed !== "object" || parsed === null) return body;
    const redacted: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(parsed)) {
      const lower = key.toLowerCase();
      const isSensitive =
        SENSITIVE_HEADER_PARTS.some((part) => lower.includes(part)) ||
        lower.includes("password") ||
        lower.includes("credential");
      redacted[key] = isSensitive ? "[REDACTED]" : value;
    }
    return JSON.stringify(redacted);
  } catch {
    return body;
  }
}

const MAX_ERROR_BODY_CHARS = 4000;
const MAX_ERROR_SUMMARY_CHARS = 240;
const MAX_ERROR_DEPTH = 6;
const MAX_ERROR_ARRAY_ITEMS = 40;
const MAX_ERROR_OBJECT_KEYS = 80;

function findHeaderKey(
  headers: Record<string, string>,
  headerName: string,
): string | undefined {
  const target = headerName.toLowerCase();
  return Object.keys(headers).find((key) => key.toLowerCase() === target);
}

function getHeaderValue(
  headers: Record<string, string>,
  headerName: string,
): string | undefined {
  const key = findHeaderKey(headers, headerName);
  return key ? headers[key] : undefined;
}

function hasNonEmptyHeader(
  headers: Record<string, string>,
  headerName: string,
): boolean {
  const value = getHeaderValue(headers, headerName);
  return typeof value === "string" && value.trim().length > 0;
}

function ensureRequiredRequestHeaders(headers: Record<string, string>): void {
  if (!hasNonEmptyHeader(headers, "x-request-id")) {
    headers["x-request-id"] = uuid();
  }

  if (!hasNonEmptyHeader(headers, "user-agent")) {
    headers["user-agent"] = CLI_USER_AGENT;
  }
}

function truncateString(value: string, maxChars: number): string {
  if (value.length <= maxChars) return value;
  return `${value.slice(0, maxChars)}…`;
}

function sanitizeErrorValue(value: unknown, depth = 0): unknown {
  if (depth > MAX_ERROR_DEPTH) {
    return "[TRUNCATED_DEPTH]";
  }

  if (typeof value === "string") {
    return truncateString(value, MAX_ERROR_BODY_CHARS);
  }

  if (
    typeof value === "number" ||
    typeof value === "boolean" ||
    value === null ||
    value === undefined
  ) {
    return value;
  }

  if (Array.isArray(value)) {
    const limited = value
      .slice(0, MAX_ERROR_ARRAY_ITEMS)
      .map((item) => sanitizeErrorValue(item, depth + 1));

    if (value.length > MAX_ERROR_ARRAY_ITEMS) {
      limited.push({
        truncated: true,
        omitted_items: value.length - MAX_ERROR_ARRAY_ITEMS,
      });
    }

    return limited;
  }

  if (typeof value === "object") {
    const sanitized: Record<string, unknown> = {};
    const entries = Object.entries(value);
    const limitedEntries = entries.slice(0, MAX_ERROR_OBJECT_KEYS);

    for (const [key, entry] of limitedEntries) {
      sanitized[key] = isSensitiveHeader(key)
        ? "[REDACTED]"
        : sanitizeErrorValue(entry, depth + 1);
    }

    if (entries.length > MAX_ERROR_OBJECT_KEYS) {
      sanitized.__truncated_keys__ = entries.length - MAX_ERROR_OBJECT_KEYS;
    }

    return sanitized;
  }

  return String(value);
}

function summarizeApiErrorBody(value: unknown): string | undefined {
  if (typeof value === "string") {
    const trimmed = value.trim();
    if (!trimmed) return undefined;
    return truncateString(trimmed, MAX_ERROR_SUMMARY_CHARS);
  }

  if (typeof value !== "object" || value === null) {
    return undefined;
  }

  const record = value as Record<string, unknown>;
  const candidates = [
    record.message,
    record.error,
    record.detail,
    record.title,
    record.code,
  ];

  for (const candidate of candidates) {
    if (typeof candidate === "string" && candidate.trim().length > 0) {
      return truncateString(candidate.trim(), MAX_ERROR_SUMMARY_CHARS);
    }
  }

  const fields = record.fields;
  if (Array.isArray(fields) && fields.length > 0) {
    const first = fields[0];
    if (typeof first === "object" && first !== null) {
      const firstRecord = first as Record<string, unknown>;
      const fieldPath =
        typeof firstRecord.path === "string" ? firstRecord.path : "field";
      const fieldMessage =
        typeof firstRecord.message === "string"
          ? firstRecord.message
          : "validation failed";
      return truncateString(
        `${fieldPath}: ${fieldMessage}`,
        MAX_ERROR_SUMMARY_CHARS,
      );
    }
  }

  return undefined;
}

function extractGraphqlErrors(value: unknown): unknown[] | undefined {
  if (typeof value !== "object" || value === null) {
    return undefined;
  }

  const errors = (value as Record<string, unknown>).errors;
  if (!Array.isArray(errors) || errors.length === 0) {
    return undefined;
  }

  return errors;
}

function summarizeGraphqlErrors(errors: unknown[]): string | undefined {
  for (const entry of errors) {
    if (typeof entry === "string" && entry.trim().length > 0) {
      return truncateString(entry.trim(), MAX_ERROR_SUMMARY_CHARS);
    }

    if (typeof entry === "object" && entry !== null) {
      const message = (entry as Record<string, unknown>).message;
      if (typeof message === "string" && message.trim().length > 0) {
        return truncateString(message.trim(), MAX_ERROR_SUMMARY_CHARS);
      }
    }
  }

  return undefined;
}

function responseRequestId(
  headers: Record<string, string>,
): string | undefined {
  return (
    headers["godaddy-request-id"] ||
    headers["x-request-id"] ||
    headers["x-amzn-requestid"]
  );
}

export type HttpMethod = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";

export interface ApiRequestOptions {
  endpoint: string;
  method?: HttpMethod;
  fields?: Record<string, string>;
  body?: string;
  headers?: Record<string, string>;
  debug?: boolean;
  graphql?: boolean;
}

export interface ApiResponse {
  status: number;
  statusText: string;
  headers: Record<string, string>;
  data: unknown;
}

/**
 * Parse field arguments into an object.
 * Fields are in the format "key=value".
 */
export function parseFieldsEffect(
  fields: string[],
): Effect.Effect<Record<string, string>, ValidationError, never> {
  return Effect.gen(function* () {
    const result: Record<string, string> = {};

    for (const field of fields) {
      const eqIndex = field.indexOf("=");
      if (eqIndex === -1) {
        return yield* Effect.fail(
          new ValidationError({
            message: `Invalid field format: ${field}`,
            userMessage: `Invalid field format: "${field}". Expected "key=value".`,
          }),
        );
      }

      const key = field.slice(0, eqIndex);
      const value = field.slice(eqIndex + 1);

      if (!key) {
        return yield* Effect.fail(
          new ValidationError({
            message: `Empty field key: ${field}`,
            userMessage: `Empty field key in: "${field}"`,
          }),
        );
      }

      result[key] = value;
    }

    return result;
  });
}

/**
 * Parse header arguments into an object.
 * Headers are in the format "Key: Value".
 */
export function parseHeadersEffect(
  headers: string[],
): Effect.Effect<Record<string, string>, ValidationError, never> {
  return Effect.gen(function* () {
    const result: Record<string, string> = {};

    for (const header of headers) {
      const colonIndex = header.indexOf(":");
      if (colonIndex === -1) {
        return yield* Effect.fail(
          new ValidationError({
            message: `Invalid header format: ${header}`,
            userMessage: `Invalid header format: "${header}". Expected "Key: Value".`,
          }),
        );
      }

      const key = header.slice(0, colonIndex).trim();
      const value = header.slice(colonIndex + 1).trim();

      if (!key) {
        return yield* Effect.fail(
          new ValidationError({
            message: `Empty header key: ${header}`,
            userMessage: `Empty header key in: "${header}"`,
          }),
        );
      }

      result[key] = value;
    }

    return result;
  });
}

/**
 * Read JSON body from file.
 */
export function readBodyFromFileEffect(
  filePath: string,
): Effect.Effect<string, ValidationError, FileSystem> {
  return Effect.gen(function* () {
    const fs = yield* FileSystem;

    const exists = yield* fileExists(filePath);
    if (!exists) {
      return yield* Effect.fail(
        new ValidationError({
          message: `File not found: ${filePath}`,
          userMessage: `File not found: ${filePath}`,
        }),
      );
    }

    const content = yield* fs.readFileString(filePath).pipe(
      Effect.mapError(
        (error) =>
          new ValidationError({
            message: `Failed to read file: ${error.message}`,
            userMessage: `Could not read file: ${filePath}`,
          }),
      ),
    );

    // Validate it's valid JSON
    try {
      JSON.parse(content);
    } catch {
      return yield* Effect.fail(
        new ValidationError({
          message: `Invalid JSON in file: ${filePath}`,
          userMessage: `File does not contain valid JSON: ${filePath}`,
        }),
      );
    }

    return content;
  });
}

/**
 * Build full URL from endpoint using the current environment.
 */
function buildUrlEffect(
  endpoint: string,
): Effect.Effect<string, CliError, FileSystem> {
  return Effect.gen(function* () {
    // Reject full URLs - only relative paths are allowed
    if (endpoint.startsWith("http://") || endpoint.startsWith("https://")) {
      return yield* Effect.fail(
        new ValidationError({
          message: "Full URLs are not allowed",
          userMessage:
            "Only relative endpoints are allowed (e.g., /v1/domains). Full URLs are not permitted.",
        }),
      );
    }

    // Get base URL from environment
    const env: Environment = yield* envGetEffect();
    const baseUrl = getApiUrl(env);

    // Ensure endpoint starts with /
    const normalizedEndpoint = endpoint.startsWith("/")
      ? endpoint
      : `/${endpoint}`;

    return `${baseUrl}${normalizedEndpoint}`;
  });
}

/**
 * Make an authenticated request to the GoDaddy API.
 */
export function apiRequestEffect(
  options: ApiRequestOptions,
): Effect.Effect<ApiResponse, CliError, FileSystem | Keychain | Fetch> {
  return Effect.gen(function* () {
    const {
      endpoint,
      method = "GET",
      fields,
      body,
      headers = {},
      debug,
      graphql = false,
    } = options;

    // Get access token with expiry info
    const tokenInfo = yield* getTokenInfoEffect().pipe(
      Effect.mapError(
        (err) =>
          new AuthenticationError({
            message: `Failed to access token from keychain: ${err.message}`,
            userMessage:
              "Unable to access secure credentials. Unlock your keychain and try again.",
          }),
      ),
    );

    if (!tokenInfo) {
      return yield* Effect.fail(
        new AuthenticationError({
          message: "No valid access token found",
          userMessage: "Not authenticated. Run 'godaddy auth login' first.",
        }),
      );
    }

    // Check if token is about to expire
    if (tokenInfo.expiresInSeconds < TOKEN_EXPIRY_BUFFER_SECONDS) {
      return yield* Effect.fail(
        new AuthenticationError({
          message: "Access token is about to expire",
          userMessage: `Token expires in ${tokenInfo.expiresInSeconds}s. Run 'godaddy auth login' to refresh.`,
        }),
      );
    }

    const accessToken = tokenInfo.accessToken;

    // Build URL
    const url = yield* buildUrlEffect(endpoint);

    // Build headers
    const requestHeaders: Record<string, string> = {
      Authorization: `Bearer ${accessToken}`,
      ...headers,
    };
    ensureRequiredRequestHeaders(requestHeaders);

    // Build body
    let requestBody: string | undefined;
    if (body) {
      requestBody = body;
      if (!hasNonEmptyHeader(requestHeaders, "content-type")) {
        requestHeaders["content-type"] = "application/json";
      }
    } else if (fields && Object.keys(fields).length > 0) {
      requestBody = JSON.stringify(fields);
      if (!hasNonEmptyHeader(requestHeaders, "content-type")) {
        requestHeaders["content-type"] = "application/json";
      }
    }

    if (debug) {
      console.error(`> ${method} ${url}`);
      const sanitizedRequestHeaders = sanitizeHeaders(requestHeaders);
      for (const [key, value] of Object.entries(sanitizedRequestHeaders)) {
        console.error(`> ${key}: ${value}`);
      }
      if (requestBody) {
        console.error(`> Body: ${redactSensitiveBodyFields(requestBody)}`);
      }
      console.error("");
    }

    // Get the HTTP client from the service context
    const fetch = yield* Fetch;

    const response = yield* Effect.tryPromise({
      try: () =>
        fetch(url, {
          method,
          headers: requestHeaders,
          body: requestBody,
        }),
      catch: (err) =>
        new NetworkError({
          message: `Network request failed: ${err instanceof Error ? err.message : String(err)}`,
          userMessage:
            "Network request failed. Check your connection and try again.",
        }),
    });

    // Parse response headers
    const responseHeaders: Record<string, string> = {};
    response.headers.forEach((value, key) => {
      responseHeaders[key] = value;
    });

    if (debug) {
      console.error(`< ${response.status} ${response.statusText}`);
      const sanitizedResponseHeaders = sanitizeHeaders(responseHeaders);
      for (const [key, value] of Object.entries(sanitizedResponseHeaders)) {
        console.error(`< ${key}: ${value}`);
      }
      console.error("");
    }

    // Parse response body
    let data: unknown;
    const contentType = response.headers.get("content-type") || "";
    if (contentType.includes("application/json")) {
      const text = yield* Effect.tryPromise({
        try: () => response.text(),
        catch: (err) =>
          new NetworkError({
            message: `Failed to read response body: ${err}`,
            userMessage: "Failed to read API response.",
          }),
      });
      if (text) {
        try {
          data = JSON.parse(text);
        } catch {
          data = text;
        }
      }
    } else {
      data = yield* Effect.tryPromise({
        try: () => response.text(),
        catch: (err) =>
          new NetworkError({
            message: `Failed to read response body: ${err}`,
            userMessage: "Failed to read API response.",
          }),
      });
    }

    if (response.ok && graphql) {
      const graphqlErrors = extractGraphqlErrors(data);
      if (graphqlErrors && graphqlErrors.length > 0) {
        const safeErrorBody = sanitizeErrorValue(data);
        const summary = summarizeGraphqlErrors(graphqlErrors);
        const requestId = responseRequestId(responseHeaders);
        const internalDetail =
          typeof safeErrorBody === "string"
            ? safeErrorBody
            : JSON.stringify(safeErrorBody);

        return yield* Effect.fail(
          new NetworkError({
            message: `GraphQL API error(s): ${internalDetail}`,
            userMessage: summary
              ? `GraphQL request returned errors: ${summary}`
              : `GraphQL request returned ${graphqlErrors.length} error(s).`,
            status: response.status,
            statusText: response.statusText,
            endpoint,
            method,
            requestId,
            responseBody: safeErrorBody,
          }),
        );
      }
    }

    // Check for error status codes
    if (!response.ok) {
      const safeErrorBody = sanitizeErrorValue(data);
      const summary = summarizeApiErrorBody(safeErrorBody);
      const requestId = responseRequestId(responseHeaders);
      const internalDetail =
        typeof safeErrorBody === "string"
          ? safeErrorBody
          : JSON.stringify(safeErrorBody);

      const context = {
        status: response.status,
        statusText: response.statusText,
        endpoint,
        method,
        requestId,
        responseBody: safeErrorBody,
      };

      // Handle 401 Unauthorized specifically - token may be revoked or invalid
      if (response.status === 401) {
        return yield* Effect.fail(
          new AuthenticationError({
            message: `Authentication failed (401): ${internalDetail}`,
            userMessage:
              "Your session has expired or is invalid. Run 'godaddy auth login' to re-authenticate.",
            ...context,
          }),
        );
      }

      // Handle 403 Forbidden - insufficient permissions
      if (response.status === 403) {
        return yield* Effect.fail(
          new AuthenticationError({
            message: `Access denied (403): ${internalDetail}`,
            userMessage:
              "You don't have permission to access this resource. Check your account permissions.",
            ...context,
          }),
        );
      }

      const userMessage =
        response.status >= 400 && response.status < 500
          ? summary
            ? `API request rejected (${response.status}): ${summary}`
            : `API request rejected with status ${response.status}: ${response.statusText}`
          : `API request failed with status ${response.status}: ${response.statusText}`;

      return yield* Effect.fail(
        new NetworkError({
          message: `API error (${response.status}): ${internalDetail}`,
          userMessage,
          ...context,
        }),
      );
    }

    return {
      status: response.status,
      statusText: response.statusText,
      headers: responseHeaders,
      data,
    };
  });
}
