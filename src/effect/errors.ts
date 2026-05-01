import * as Data from "effect/Data";

/**
 * Structured per-field validation detail, mirroring the upstream
 * source-of-truth `ErrorField` shape. Forwarded from the GraphQL
 * `extensions.fields` entries when a mutation returns a validation error.
 *
 * All fields are optional — the wire boundary is defensive, and every
 * consumer re-validates before use (see `graphql-error.ts`).
 */
export interface ErrorField {
  readonly code?: string;
  readonly message?: string;
  readonly path?: string;
  readonly pathRelated?: string;
  readonly value?: unknown;
}

/**
 * Client-side input validation failure — "we rejected your input before
 * sending it." Raised by arktype when a command argument fails the
 * service input schema. Deliberately does NOT carry `ApiErrorContext`,
 * because no request reached the server.
 *
 * Server-side validation failures (the server rejected the request) are
 * represented by `ServerError { kind: "VALIDATION" }`.
 */
export class ValidationError extends Data.TaggedError("ValidationError")<{
  readonly message: string;
  readonly userMessage: string;
  readonly fields?: ReadonlyArray<ErrorField>;
}> {}

export interface ApiErrorContext {
  readonly status?: number;
  readonly statusText?: string;
  readonly endpoint?: string;
  readonly method?: string;
  readonly requestId?: string;
  readonly responseBody?: unknown;
}

export class NetworkError extends Data.TaggedError("NetworkError")<
  {
    readonly message: string;
    readonly userMessage: string;
  } & ApiErrorContext
> {}

export class AuthenticationError extends Data.TaggedError(
  "AuthenticationError",
)<
  {
    readonly message: string;
    readonly userMessage: string;
  } & ApiErrorContext
> {}

/**
 * Classification family for server-side failures with known semantics.
 * Each kind maps to a stable envelope `code` and `fix` string (see
 * `cli/agent/errors.ts`); the server's original `extensions.code` is
 * always preserved under `details.response` for agents that need finer
 * discrimination.
 */
export type ServerErrorKind =
  | "VALIDATION"
  | "FORBIDDEN"
  | "NOT_FOUND"
  | "CONFLICT"
  | "RATE_LIMITED";

/**
 * Server-side failure with a known classification. Replaces the former
 * per-kind classes (`ForbiddenError`, `NotFoundError`, `ConflictError`,
 * `RateLimitError`, and server-side validation failures): those were
 * structurally identical and no consumer discriminated on `_tag` in a
 * type-driven way. One `ServerError` with a `kind` discriminant keeps
 * the taxonomy open to extension without adding a class per code.
 */
export class ServerError extends Data.TaggedError("ServerError")<
  {
    readonly kind: ServerErrorKind;
    readonly message: string;
    readonly userMessage: string;
    readonly fields?: ReadonlyArray<ErrorField>;
  } & ApiErrorContext
> {}

export class ConfigurationError extends Data.TaggedError("ConfigurationError")<{
  readonly message: string;
  readonly userMessage: string;
}> {}

export class SecurityError extends Data.TaggedError("SecurityError")<{
  readonly message: string;
  readonly userMessage: string;
}> {}

export type CliError =
  | ValidationError
  | NetworkError
  | AuthenticationError
  | ServerError
  | ConfigurationError
  | SecurityError;

/**
 * Envelope `code` for a given `ServerErrorKind`. Aligned with the
 * server's wire codes where feasible (`RATE_LIMITED` →
 * `RATE_LIMIT_EXCEEDED`) so agents that speak the server taxonomy don't
 * have to translate.
 */
const SERVER_ERROR_CODE: Record<ServerErrorKind, string> = {
  VALIDATION: "VALIDATION_ERROR",
  FORBIDDEN: "FORBIDDEN",
  NOT_FOUND: "NOT_FOUND",
  CONFLICT: "CONFLICT",
  RATE_LIMITED: "RATE_LIMIT_EXCEEDED",
};

export function errorCode(error: CliError): string {
  switch (error._tag) {
    case "ValidationError":
      return "VALIDATION_ERROR";
    case "NetworkError":
      return "NETWORK_ERROR";
    case "AuthenticationError":
      // NOTE: `cli/agent/errors.ts` overrides this to `AUTH_REQUIRED` in
      // the envelope for historical compatibility with existing agent
      // consumers; aligning the two values is tracked as a follow-up.
      return "AUTH_ERROR";
    case "ServerError":
      return SERVER_ERROR_CODE[error.kind];
    case "ConfigurationError":
      return "CONFIG_ERROR";
    case "SecurityError":
      return "SECURITY_BLOCKED";
  }
}
