import {
  type ApiErrorContext,
  type CliError,
  type ServerError,
  type ServerErrorKind,
  type ValidationError,
  errorCode,
} from "@/effect/errors";
import * as HelpDoc from "@effect/cli/HelpDoc";
import type { ValidationError as EffectValidationError } from "@effect/cli/ValidationError";

export interface AgentErrorDetails {
  message: string;
  code: string;
  fix: string;
  details?: Record<string, unknown>;
}

const ANSI_ESCAPE_PATTERN = new RegExp(
  `${String.fromCharCode(27)}\\[[0-9;]*m`,
  "g",
);

function stripAnsi(value: string): string {
  return value.replace(ANSI_ESCAPE_PATTERN, "");
}

function formatValidationMessage(error: EffectValidationError): string {
  if ("error" in error && error.error) {
    const text = stripAnsi(HelpDoc.toAnsiText(error.error)).trim();
    if (text.length > 0) {
      return text;
    }
  }

  return "Invalid command input";
}

function apiDetails(error: CliError): Record<string, unknown> | undefined {
  const context = error as Partial<ApiErrorContext>;

  const details: Record<string, unknown> = {};
  if (typeof context.status === "number") {
    details.status = context.status;
  }
  if (typeof context.statusText === "string" && context.statusText.length > 0) {
    details.status_text = context.statusText;
  }
  if (typeof context.endpoint === "string" && context.endpoint.length > 0) {
    details.endpoint = context.endpoint;
  }
  if (typeof context.method === "string" && context.method.length > 0) {
    details.method = context.method;
  }
  if (typeof context.requestId === "string" && context.requestId.length > 0) {
    details.request_id = context.requestId;
  }
  if (context.responseBody !== undefined) {
    details.response = context.responseBody;
  }

  return Object.keys(details).length > 0 ? details : undefined;
}

function hasGraphqlErrors(
  details: Record<string, unknown> | undefined,
): boolean {
  const response = details?.response;
  if (typeof response !== "object" || response === null) {
    return false;
  }

  const errors = (response as Record<string, unknown>).errors;
  return Array.isArray(errors) && errors.length > 0;
}

function fixForNetworkError(
  details: Record<string, unknown> | undefined,
): string {
  if (hasGraphqlErrors(details)) {
    return "Check GraphQL query, variables, and operationName. Inspect error.details.response.errors for resolver/validation details.";
  }

  const status = details?.status;
  if (typeof status === "number") {
    if (status >= 400 && status < 500) {
      return "Check request path/query/body. Inspect error.details.response for API validation feedback.";
    }
    if (status >= 500) {
      return "The API is currently failing server-side. Retry, or check service health/incidents.";
    }
  }

  return "Verify environment connectivity with: godaddy env get and retry.";
}

/** Envelope `fix` string per `ServerErrorKind`. */
const SERVER_FIX: Record<ServerErrorKind, string> = {
  VALIDATION:
    "Review the per-field issues in error.details.fields (and error.details.response for the raw server payload) and retry with valid values.",
  FORBIDDEN:
    "Your account or token lacks permission for this operation. Contact your org admin, or switch environments with: godaddy env use <environment>.",
  NOT_FOUND:
    "Use 'godaddy application list' to find the correct name/id, or create the resource first.",
  CONFLICT:
    "Resolve the conflicting state shown in error.details.response (see errors[].extensions.fields for the specific field/value) and retry.",
  RATE_LIMITED:
    "Rate limit exceeded. Retry later; inspect error.details.response for any retry-after hint from the server.",
};

function fromTaggedError(error: CliError): AgentErrorDetails {
  const code = errorCode(error);
  const message = error.userMessage || error.message;
  const details = apiDetails(error);

  switch (error._tag) {
    case "ValidationError": {
      // Client-side (arktype) validation failure — no request was sent.
      const fields = (error as ValidationError).fields;
      const hasFields = Array.isArray(fields) && fields.length > 0;
      return {
        message,
        code,
        fix: hasFields
          ? "Review the per-field issues in error.details.fields and retry with valid values."
          : "Review command arguments and try again with valid values.",
        details: hasFields ? { fields } : undefined,
      };
    }
    case "AuthenticationError":
      // Envelope emits `AUTH_REQUIRED` (not `errorCode()`'s `AUTH_ERROR`)
      // for historical compatibility with existing agent consumers.
      // Aligning the two values is tracked as a follow-up.
      return {
        message,
        code: "AUTH_REQUIRED",
        fix: "Run: godaddy auth login",
        details,
      };
    case "ServerError": {
      const server = error as ServerError;
      const hasFields =
        Array.isArray(server.fields) && server.fields.length > 0;
      const mergedDetails =
        hasFields && server.kind === "VALIDATION"
          ? { ...(details ?? {}), fields: server.fields }
          : details;
      return {
        message,
        code,
        fix: SERVER_FIX[server.kind],
        details: mergedDetails,
      };
    }
    case "ConfigurationError":
      return {
        message,
        code,
        fix: "Check your config with: godaddy env info [environment]",
      };
    case "NetworkError":
      return {
        message,
        code,
        fix: fixForNetworkError(details),
        details,
      };
    case "SecurityError":
      return {
        message,
        code,
        fix: "Resolve security findings and rerun: godaddy application deploy <name>",
      };
  }
}

function inferFromMessage(message: string): AgentErrorDetails {
  const lower = message.toLowerCase();

  if (lower.includes("--output")) {
    return {
      message,
      code: "UNSUPPORTED_OPTION",
      fix: "Remove --output; all commands now emit JSON envelopes.",
    };
  }

  if (lower.includes("security") || lower.includes("blocked")) {
    return {
      message,
      code: "SECURITY_BLOCKED",
      fix: "Resolve security findings and rerun: godaddy application deploy <name>",
    };
  }

  if (lower.includes("not found") || lower.includes("does not exist")) {
    return {
      message,
      code: "NOT_FOUND",
      fix: "Use discovery commands such as: godaddy application list or godaddy actions list.",
    };
  }

  if (lower.includes("auth") || lower.includes("token")) {
    return {
      message,
      code: "AUTH_REQUIRED",
      fix: "Run: godaddy auth login",
    };
  }

  return {
    message,
    code: "UNEXPECTED_ERROR",
    fix: "Run: godaddy for command discovery and retry with corrected input.",
  };
}

function isTaggedError(error: unknown): error is CliError {
  return (
    typeof error === "object" &&
    error !== null &&
    "_tag" in error &&
    typeof (error as { _tag: unknown })._tag === "string" &&
    [
      "ValidationError",
      "NetworkError",
      "AuthenticationError",
      "ServerError",
      "ConfigurationError",
      "SecurityError",
    ].includes((error as { _tag: string })._tag)
  );
}

export function mapRuntimeError(error: unknown): AgentErrorDetails {
  if (isTaggedError(error)) {
    return fromTaggedError(error);
  }

  if (error instanceof Error) {
    return inferFromMessage(error.message);
  }

  return {
    message: "Unknown error",
    code: "UNEXPECTED_ERROR",
    fix: "Run: godaddy for command discovery.",
  };
}

export function mapValidationError(
  error: EffectValidationError,
): AgentErrorDetails {
  const message = formatValidationMessage(error);

  if (message.includes("--output")) {
    return {
      message,
      code: "UNSUPPORTED_OPTION",
      fix: "Remove --output; all commands now emit JSON envelopes.",
    };
  }

  switch (error._tag) {
    case "CommandMismatch":
      return {
        message,
        code: "COMMAND_NOT_FOUND",
        fix: "Run: godaddy",
      };
    case "MissingFlag":
    case "MissingValue":
    case "InvalidArgument":
    case "InvalidValue":
    case "MultipleValuesDetected":
    case "NoBuiltInMatch":
    case "UnclusteredFlag":
    case "MissingSubcommand":
    case "CorrectedFlag":
      return {
        message,
        code: "VALIDATION_ERROR",
        fix: "Provide valid arguments/options shown in --help and retry.",
      };
    case "HelpRequested":
      return {
        message,
        code: "VALIDATION_ERROR",
        fix: "Use --help for command usage details.",
      };
    default:
      return {
        message,
        code: "VALIDATION_ERROR",
        fix: "Check command usage with --help and retry.",
      };
  }
}
