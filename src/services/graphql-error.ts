/**
 * Shared GraphQL error-to-CLI-error mapper.
 *
 * Used by every service module that talks to the upstream source of
 * truth (`applications.ts`, `extension/presigned-url.ts`, future additions).
 * Keeping the classification logic here guarantees every GraphQL call
 * produces envelopes with consistent top-level `code` + `fix` values.
 *
 * Classification is driven by an explicit `CODE_MAP` — codes we know
 * about are routed to `ServerError { kind }`; everything else falls
 * through to `NetworkError` with full HTTP context preserved so agents
 * can still discriminate via the server's original `extensions.code` in
 * `details.response`. Opt-in classification (no `endsWith` wildcards)
 * prevents codes like `POLICY_NOT_FOUND_IN_CACHE` from being captured
 * incorrectly.
 *
 * Defensive posture: every wire-shape assumption is runtime-guarded
 * (`typeof`, `Array.isArray`) so a server-side response reshuffle or
 * minor shape drift surfaces as a `NetworkError` fallback, never as a
 * thrown exception inside the CLI.
 */

import {
  type ApiErrorContext,
  AuthenticationError,
  type CliError,
  type ErrorField,
  NetworkError,
  ServerError,
  type ServerErrorKind,
} from "@/effect/errors";
import { ClientError } from "graphql-request";

/** Untyped wire shape for a single GraphQL error entry. */
type GraphQLErrorEntry = {
  message?: string;
  extensions?: { code?: unknown; fields?: unknown };
};

/**
 * Explicit server-code → classification kind. Sourced from the upstream
 * source-of-truth error taxonomy. New codes must be added here
 * explicitly; unknown codes fall through to `NetworkError` with the
 * server's original code preserved under `details.response`.
 *
 * `AUTH_ERROR` / `UNAUTHORIZED` route to `AuthenticationError` (handled
 * separately in `mapGraphQLError`) rather than through this map.
 */
const CODE_MAP: Readonly<Record<string, ServerErrorKind>> = {
  VALIDATION_ERROR: "VALIDATION",
  FORBIDDEN: "FORBIDDEN",
  INSUFFICIENT_PERMISSIONS: "FORBIDDEN",
  NOT_FOUND: "NOT_FOUND",
  APPLICATION_NOT_FOUND: "NOT_FOUND",
  RELEASE_NOT_FOUND: "NOT_FOUND",
  SUBSCRIPTION_NOT_FOUND: "NOT_FOUND",
  ACTION_NOT_FOUND: "NOT_FOUND",
  POLICY_NOT_FOUND: "NOT_FOUND",
  DUPLICATE_CONFLICT: "CONFLICT",
  STATE_CONFLICT: "CONFLICT",
  VERSION_CONFLICT: "CONFLICT",
  RATE_LIMIT_EXCEEDED: "RATE_LIMITED",
  RATE_LIMIT: "RATE_LIMITED",
  INVALID_FIELD: "VALIDATION",
};

/**
 * Normalize an untrusted `errors` value to a safe array. Protects every
 * downstream array method (`find`, `map`, index access) from crashing
 * when the server returns a non-array shape (`null`, a string, an object).
 */
function toErrorArray(
  value: unknown,
): ReadonlyArray<GraphQLErrorEntry | undefined> {
  return Array.isArray(value)
    ? (value as ReadonlyArray<GraphQLErrorEntry | undefined>)
    : [];
}

/**
 * Pick the most informative error entry from a GraphQL response.
 *   1. First error with a string `extensions.code`.
 *   2. First error with a non-empty `extensions.fields` array.
 *   3. `errors[0]` as a last resort.
 *
 * Called once at the top of `mapGraphQLError`; both message extraction
 * and classification use the same entry by construction.
 */
function pickPrimaryError(
  errors: ReadonlyArray<GraphQLErrorEntry | undefined>,
): GraphQLErrorEntry | undefined {
  const withCode = errors.find((e) => typeof e?.extensions?.code === "string");
  if (withCode) return withCode;
  const withFields = errors.find((e) => {
    const fields = e?.extensions?.fields;
    return Array.isArray(fields) && fields.length > 0;
  });
  return withFields ?? errors[0];
}

/** Developer-facing `message`: `"<server message> (<CODE>)"` when classified. */
function messageFromPrimary(primary: GraphQLErrorEntry | undefined): string {
  if (!primary) return "An unexpected error occurred";
  const code = primary.extensions?.code;
  const suffix = typeof code === "string" ? ` (${code})` : "";
  return `${primary.message ?? "GraphQL error"}${suffix}`;
}

/**
 * Server's per-entry message, if it gave us one. Both the classified
 * and `NetworkError` paths prefer this when present.
 */
function primaryMessageOrUndefined(
  primary: GraphQLErrorEntry | undefined,
): string | undefined {
  return typeof primary?.message === "string" && primary.message.length > 0
    ? primary.message
    : undefined;
}

/**
 * User-facing message for an *unclassified* failure (`NetworkError`
 * fallback path). HTTP status drives a canned remediation hint when no
 * per-entry message is available.
 *
 * Classified errors deliberately do NOT use this — their `fix` string
 * already carries the status-appropriate guidance, and a status-derived
 * `userMessage` could contradict the classification (e.g. a
 * `ServerError { kind: "FORBIDDEN" }` returned over HTTP 401 would
 * otherwise pick up "Run godaddy auth login").
 */
function networkUserMessage(
  primary: GraphQLErrorEntry | undefined,
  status: number | undefined,
): string {
  const fromPrimary = primaryMessageOrUndefined(primary);
  if (fromPrimary !== undefined) return fromPrimary;
  if (status === 401) return "Authentication failed. Run 'godaddy auth login'.";
  if (status === 403)
    return "Access denied. You may not have permission for this operation in the current environment.";
  if (status === 404) return "The requested resource was not found.";
  if (typeof status === "number" && status >= 500)
    return "The server encountered an error. Please try again later.";
  return "An unexpected error occurred";
}

/** Pull `ErrorField[]` off a primary error, guarded at runtime. */
function fieldsFromPrimary(
  primary: GraphQLErrorEntry | undefined,
): ReadonlyArray<ErrorField> | undefined {
  const raw = primary?.extensions?.fields;
  return Array.isArray(raw) ? (raw as ReadonlyArray<ErrorField>) : undefined;
}

/**
 * Populate `ApiErrorContext` from a graphql-request `ClientError`.
 * Surfacing `status` + `responseBody` lets `cli/agent/errors.ts` →
 * `fixForNetworkError` produce tailored GraphQL fix hints and preserves
 * the server's original `extensions.code` in `details.response` for
 * agents that want finer-grained classification than the CLI taxonomy.
 *
 * `responseBody` is shaped as `{ errors, data }` (matching a standard
 * GraphQL response body) because the agent-side `hasGraphqlErrors`
 * detector looks for `.errors` under `details.response`. Headers and
 * other non-body fields are intentionally excluded to avoid
 * `Headers`-instance serialization quirks.
 */
function networkContextFromClientError(err: ClientError): ApiErrorContext {
  return {
    status: err.response.status,
    responseBody: {
      errors: toErrorArray(err.response.errors),
      data: err.response.data ?? null,
    },
  };
}

/**
 * Classify a primary error entry into a `ServerErrorKind`, an
 * `AuthenticationError`, or `null` for "unknown / fall through to
 * `NetworkError`". Looks at the top-level `extensions.code` first, then
 * any codes inside `extensions.fields[]` (the source of truth emits
 * some classifications only at the field level, e.g. `INVALID_FIELD`,
 * `RATE_LIMIT`).
 */
function classify(
  primary: GraphQLErrorEntry | undefined,
  fields: ReadonlyArray<ErrorField> | undefined,
): ServerErrorKind | "AUTHENTICATION" | null {
  const code = primary?.extensions?.code;
  if (typeof code === "string") {
    if (code === "AUTH_ERROR" || code === "UNAUTHORIZED") {
      return "AUTHENTICATION";
    }
    const kind = CODE_MAP[code];
    if (kind) return kind;
  }
  if (fields) {
    for (const f of fields) {
      const fc = f?.code;
      if (typeof fc === "string") {
        const kind = CODE_MAP[fc];
        if (kind) return kind;
      }
    }
  }
  return null;
}

/**
 * Map a GraphQL / transport error to the correct CLI error class.
 *
 * Emits:
 *   - `ServerError { kind }` for classified server failures
 *     (`VALIDATION`, `FORBIDDEN`, `NOT_FOUND`, `CONFLICT`, `RATE_LIMITED`).
 *   - `AuthenticationError` for `AUTH_ERROR` / `UNAUTHORIZED`.
 *   - `NetworkError` for transport failures and any unclassified server
 *     response (full HTTP context preserved in either case).
 */
export function mapGraphQLError(err: unknown): CliError {
  if (!(err instanceof ClientError)) {
    return new NetworkError({
      message: "An unexpected error occurred",
      userMessage: "An unexpected error occurred",
    });
  }

  const primary = pickPrimaryError(toErrorArray(err.response.errors));
  const message = messageFromPrimary(primary);
  const ctx = networkContextFromClientError(err);
  const fields = fieldsFromPrimary(primary);
  const kind = classify(primary, fields);

  // For classified errors, the user-facing message is just the server's
  // own message (the `fix` string carries remediation guidance). The
  // `NetworkError` fallback is the only path that uses HTTP-status-based
  // canned strings — so a classification can never be contradicted by
  // its own user message.
  const classifiedUserMessage =
    primaryMessageOrUndefined(primary) ?? "An unexpected error occurred";

  if (kind === "AUTHENTICATION") {
    return new AuthenticationError({
      message,
      userMessage: classifiedUserMessage,
      ...ctx,
    });
  }
  if (kind !== null) {
    return new ServerError({
      kind,
      message,
      userMessage: classifiedUserMessage,
      fields,
      ...ctx,
    });
  }
  return new NetworkError({
    message,
    userMessage: networkUserMessage(primary, err.response.status),
    ...ctx,
  });
}
