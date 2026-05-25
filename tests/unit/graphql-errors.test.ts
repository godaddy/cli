import * as Exit from "effect/Exit";
import { HttpResponse, graphql } from "msw";
import { describe, expect, test } from "vitest";
import { mapRuntimeError } from "../../src/cli/agent/errors";
import type {
  AuthenticationError,
  CliError,
  NetworkError,
  ServerError,
} from "../../src/effect/errors";
import { getApplicationEffect } from "../../src/services/applications";
import { extractFailure, runEffectExit } from "../setup/effect-test-utils";
import { server } from "../setup/msw-server";
import { withNoAuth, withValidAuth } from "../setup/test-utils";

// Test shape convention: `expect(err.status).toBe(200)` on error paths
// is intentional — GraphQL conventionally returns HTTP 200 with an
// `errors[]` body, and the mapper preserves that status in
// `ApiErrorContext` as the transport-level signal.

// `message` is optional on the wire — real GraphQL responses can carry
// errors with only `extensions`, and several tests exercise that path.
type GraphQLMockError = {
  message?: string;
  extensions?: Record<string, unknown>;
};

function mockGraphQLErrorResponse(
  error: GraphQLMockError | GraphQLMockError[],
  status = 200,
) {
  const errors = Array.isArray(error) ? error : [error];
  server.use(
    graphql.operation(() =>
      HttpResponse.json({ data: null, errors }, { status }),
    ),
  );
}

describe("GraphQL Error Handling", () => {
  test("handles authentication error", async () => {
    withNoAuth();

    const exit = await runEffectExit(
      getApplicationEffect("test-app-1", { accessToken: null }),
    );
    const err = extractFailure(exit) as { message: string };
    expect(err.message).toContain("Access token is required");
  });

  test("handles validation errors", async () => {
    withValidAuth();

    mockGraphQLErrorResponse({
      message: "Validation failed",
      extensions: {
        code: "VALIDATION_ERROR",
        fieldErrors: { name: ["Name is required"] },
      },
    });

    const exit = await runEffectExit(
      getApplicationEffect("", { accessToken: "test-token-123" }),
    );
    const err = extractFailure(exit) as { message: string };
    expect(err.message).toContain("Validation failed");
  });

  test("handles server errors", async () => {
    withValidAuth();

    mockGraphQLErrorResponse({ message: "Internal server error" });

    const exit = await runEffectExit(
      getApplicationEffect("test-app-1", { accessToken: "test-token-123" }),
    );
    const err = extractFailure(exit) as { message: string };
    expect(err.message).toContain("Internal server error");
  });

  test("handles network errors", async () => {
    withValidAuth();

    server.use(
      graphql.operation(() => {
        return HttpResponse.error();
      }),
    );

    const exit = await runEffectExit(
      getApplicationEffect("test-app-1", { accessToken: "test-token-123" }),
    );
    expect(Exit.isFailure(exit)).toBe(true);
  });
});

describe("GraphQL error → CLI error class mapping", () => {
  // Server-side errors carry a classification in extensions.code. The CLI
  // should preserve that classification so the JSON envelope reports a
  // useful code and fix string, rather than collapsing every server error
  // into NETWORK_ERROR.

  test("VALIDATION_ERROR extensions code → ServerError { kind: 'VALIDATION' }", async () => {
    withValidAuth();

    mockGraphQLErrorResponse({
      message: "Validation failed: url: Invalid URL",
      extensions: { code: "VALIDATION_ERROR" },
    });

    const exit = await runEffectExit(
      getApplicationEffect("x", { accessToken: "test-token-123" }),
    );
    const err = extractFailure(exit) as ServerError;
    expect(err._tag).toBe("ServerError");
    expect(err.kind).toBe("VALIDATION");
    expect(err.message).toContain("Validation failed");

    const envelope = mapRuntimeError(err);
    expect(envelope.code).toBe("VALIDATION_ERROR");
  });

  test("VALIDATION_ERROR forwards extensions.fields onto ServerError", async () => {
    withValidAuth();

    const fields = [
      {
        code: "INVALID_FIELD",
        message: "URL must use HTTPS protocol",
        path: "url",
        value: "http://example.com",
      },
      {
        code: "INVALID_FIELD",
        message: "Name must not be empty",
        path: "name",
      },
    ];

    mockGraphQLErrorResponse({
      message: "Validation failed",
      extensions: { code: "VALIDATION_ERROR", fields },
    });

    const exit = await runEffectExit(
      getApplicationEffect("x", { accessToken: "test-token-123" }),
    );
    const err = extractFailure(exit) as ServerError;
    expect(err._tag).toBe("ServerError");
    expect(err.kind).toBe("VALIDATION");
    expect(err.fields).toEqual(fields);

    // Fields must also surface in the agent envelope under details.fields.
    const envelope = mapRuntimeError(err);
    expect(envelope.details?.fields).toEqual(fields);
  });

  test.each(["AUTH_ERROR", "UNAUTHORIZED"])(
    "%s extensions code → AuthenticationError",
    async (code) => {
      withValidAuth();

      mockGraphQLErrorResponse({
        message: `Denied: ${code}`,
        extensions: { code },
      });

      const exit = await runEffectExit(
        getApplicationEffect("x", { accessToken: "test-token-123" }),
      );
      const err = extractFailure(exit) as AuthenticationError;
      expect(err._tag).toBe("AuthenticationError");
      expect(err.status).toBe(200);
      expect(err.responseBody).toBeDefined();

      const envelope = mapRuntimeError(err);
      expect(envelope.code).toBe("AUTH_REQUIRED");
      expect(envelope.fix).toContain("godaddy auth login");
    },
  );

  test.each(["FORBIDDEN", "INSUFFICIENT_PERMISSIONS"])(
    "%s extensions code → ServerError { kind: 'FORBIDDEN' }",
    async (code) => {
      // FORBIDDEN / INSUFFICIENT_PERMISSIONS are authorization failures —
      // caller is authenticated but lacks the required permission. The
      // envelope must point at a permission fix, not a re-login.
      withValidAuth();

      mockGraphQLErrorResponse({
        message: `Denied: ${code}`,
        extensions: { code },
      });

      const exit = await runEffectExit(
        getApplicationEffect("x", { accessToken: "test-token-123" }),
      );
      const err = extractFailure(exit) as ServerError;
      expect(err._tag).toBe("ServerError");
      expect(err.kind).toBe("FORBIDDEN");
      expect(err.status).toBe(200);
      expect(err.responseBody).toBeDefined();

      const envelope = mapRuntimeError(err);
      expect(envelope.code).toBe("FORBIDDEN");
      expect(envelope.fix).toMatch(/permission/i);
    },
  );

  test.each(["NOT_FOUND", "APPLICATION_NOT_FOUND", "RELEASE_NOT_FOUND"])(
    "%s extensions code → ServerError { kind: 'NOT_FOUND' }",
    async (code) => {
      // The source of truth emits `code: "NOT_FOUND"` at the top level and
      // `<RESOURCE>_NOT_FOUND` in fields[].code. The explicit CODE_MAP
      // enumerates both forms so either classifies correctly.
      withValidAuth();

      mockGraphQLErrorResponse({
        message: "Resource not found",
        extensions: { code },
      });

      const exit = await runEffectExit(
        getApplicationEffect("x", { accessToken: "test-token-123" }),
      );
      const err = extractFailure(exit) as ServerError;
      expect(err._tag).toBe("ServerError");
      expect(err.kind).toBe("NOT_FOUND");
      expect(err.status).toBe(200);
      expect(err.responseBody).toBeDefined();

      const envelope = mapRuntimeError(err);
      expect(envelope.code).toBe("NOT_FOUND");
      expect(envelope.fix).toContain("godaddy application list");
    },
  );

  test.each(["DUPLICATE_CONFLICT", "STATE_CONFLICT", "VERSION_CONFLICT"])(
    "%s extensions code → ServerError { kind: 'CONFLICT' }",
    async (code) => {
      withValidAuth();

      mockGraphQLErrorResponse({
        message: `Conflict: ${code}`,
        extensions: { code },
      });

      const exit = await runEffectExit(
        getApplicationEffect("x", { accessToken: "test-token-123" }),
      );
      const err = extractFailure(exit) as ServerError;
      expect(err._tag).toBe("ServerError");
      expect(err.kind).toBe("CONFLICT");
      expect(err.status).toBe(200);
      expect(err.responseBody).toBeDefined();

      const envelope = mapRuntimeError(err);
      expect(envelope.code).toBe("CONFLICT");
      expect(envelope.fix).toMatch(/conflict/i);
    },
  );

  test.each(["RATE_LIMIT_EXCEEDED", "RATE_LIMIT"])(
    "%s extensions code → ServerError { kind: 'RATE_LIMITED' }",
    async (code) => {
      // RateLimitError emits `code: "RATE_LIMIT_EXCEEDED"` at top level and
      // `code: "RATE_LIMIT"` in fields[]. The envelope code matches the
      // server's wire code (`RATE_LIMIT_EXCEEDED`) so agents that speak
      // the server taxonomy don't have to translate.
      withValidAuth();

      mockGraphQLErrorResponse({
        message: "Rate limited",
        extensions: { code },
      });

      const exit = await runEffectExit(
        getApplicationEffect("x", { accessToken: "test-token-123" }),
      );
      const err = extractFailure(exit) as ServerError;
      expect(err._tag).toBe("ServerError");
      expect(err.kind).toBe("RATE_LIMITED");
      expect(err.status).toBe(200);
      expect(err.responseBody).toBeDefined();

      const envelope = mapRuntimeError(err);
      expect(envelope.code).toBe("RATE_LIMIT_EXCEEDED");
      expect(envelope.fix).toMatch(/rate limit/i);
    },
  );

  test("unknown extensions code → NetworkError with HTTP context preserved", async () => {
    withValidAuth();

    mockGraphQLErrorResponse({
      message: "Something went wrong",
      extensions: { code: "OAUTH_CLIENT_UPSTREAM_ERROR" },
    });

    const exit = await runEffectExit(
      getApplicationEffect("x", { accessToken: "test-token-123" }),
    );
    const err = extractFailure(exit) as NetworkError;
    expect(err._tag).toBe("NetworkError");
    expect(err.status).toBe(200);
    // Original server code is preserved so agents can still discriminate.
    expect(JSON.stringify(err.responseBody)).toContain(
      "OAUTH_CLIENT_UPSTREAM_ERROR",
    );
  });

  test("no extensions code → NetworkError (default) with HTTP context", async () => {
    withValidAuth();

    mockGraphQLErrorResponse({ message: "Internal server error" });

    const exit = await runEffectExit(
      getApplicationEffect("x", { accessToken: "test-token-123" }),
    );
    const err = extractFailure(exit) as NetworkError;
    expect(err._tag).toBe("NetworkError");
    expect(err.status).toBe(200);
    expect(err.responseBody).toBeDefined();
  });

  test("classified error is picked even when errors[0] lacks a code", async () => {
    // Regression: `message`, `_tag`, and `kind` must all come from the same
    // error entry. A prior implementation read `message` from `errors[0]`
    // while picking classification from a later entry, producing a correctly
    // classified CLI error with an unrelated message attached. The pick-once
    // refactor in `mapGraphQLError` makes that invariant structural.
    withValidAuth();

    mockGraphQLErrorResponse([
      { message: "Partial failure without a code" },
      {
        message: "Validation failed: url: Invalid URL",
        extensions: { code: "VALIDATION_ERROR" },
      },
    ]);

    const exit = await runEffectExit(
      getApplicationEffect("x", { accessToken: "test-token-123" }),
    );
    const err = extractFailure(exit) as ServerError;
    expect(err._tag).toBe("ServerError");
    expect(err.kind).toBe("VALIDATION");
    expect(err.message).toContain("Validation failed: url: Invalid URL");
    expect(err.message).toContain("VALIDATION_ERROR");
    expect(err.message).not.toContain("Partial failure without a code");
  });

  test("no error carries a code → NetworkError with errors[0].message", async () => {
    // Sub-case of the above: when nothing classifies, fall back to errors[0]
    // consistently for both tag and message.
    withValidAuth();

    mockGraphQLErrorResponse([
      { message: "First failure without a code" },
      { message: "Second failure also without a code" },
    ]);

    const exit = await runEffectExit(
      getApplicationEffect("x", { accessToken: "test-token-123" }),
    );
    const err = extractFailure(exit) as CliError;
    expect(err._tag).toBe("NetworkError");
    expect(err.message).toContain("First failure without a code");
    expect(err.message).not.toContain("Second failure");
  });

  test("non-array extensions.fields does not crash the mapper", async () => {
    // Defensive: if the server sends a malformed `fields` (e.g. string or
    // object instead of array), classification must not throw inside `.map`.
    // Falls through to NetworkError since no classification can be derived.
    withValidAuth();

    mockGraphQLErrorResponse({
      message: "Shape drift from upstream",
      extensions: {
        fields: "not-an-array" as unknown as Record<string, unknown>,
      },
    });

    const exit = await runEffectExit(
      getApplicationEffect("x", { accessToken: "test-token-123" }),
    );
    const err = extractFailure(exit) as CliError;
    expect(err._tag).toBe("NetworkError");
    expect(err.message).toContain("Shape drift from upstream");
  });

  test.each<{ fieldCode: string; expectedKind: ServerError["kind"] }>([
    { fieldCode: "INVALID_FIELD", expectedKind: "VALIDATION" },
    { fieldCode: "INSUFFICIENT_PERMISSIONS", expectedKind: "FORBIDDEN" },
    { fieldCode: "APPLICATION_NOT_FOUND", expectedKind: "NOT_FOUND" },
    { fieldCode: "STATE_CONFLICT", expectedKind: "CONFLICT" },
    { fieldCode: "RATE_LIMIT", expectedKind: "RATE_LIMITED" },
  ])(
    "field-level code $fieldCode classifies to ServerError { kind: $expectedKind }",
    async ({ fieldCode, expectedKind }) => {
      // The source of truth emits some classifications only via
      // `extensions.fields[].code`, with no top-level `extensions.code`.
      // The mapper must honor both placements.
      withValidAuth();

      mockGraphQLErrorResponse({
        message: `Field-level ${fieldCode}`,
        extensions: {
          fields: [{ code: fieldCode, message: "details", path: "x" }],
        },
      });

      const exit = await runEffectExit(
        getApplicationEffect("x", { accessToken: "test-token-123" }),
      );
      const err = extractFailure(exit) as ServerError;
      expect(err._tag).toBe("ServerError");
      expect(err.kind).toBe(expectedKind);
    },
  );

  test("unknown code ending in _NOT_FOUND does NOT mis-classify", async () => {
    // The old `endsWith('_NOT_FOUND')` heuristic would capture codes like
    // `POLICY_NOT_FOUND_IN_CACHE` that aren't genuine resource-not-found
    // failures. The explicit CODE_MAP keeps classification opt-in; novel
    // codes fall through to NetworkError with context preserved.
    withValidAuth();

    mockGraphQLErrorResponse({
      message: "Cache miss",
      extensions: { code: "POLICY_NOT_FOUND_IN_CACHE" },
    });

    const exit = await runEffectExit(
      getApplicationEffect("x", { accessToken: "test-token-123" }),
    );
    const err = extractFailure(exit) as NetworkError;
    expect(err._tag).toBe("NetworkError");
    expect(JSON.stringify(err.responseBody)).toContain(
      "POLICY_NOT_FOUND_IN_CACHE",
    );
  });

  test("classified userMessage never contradicts the classification (status-canned strings stay on NetworkError fallback)", async () => {
    // Regression: previously, a single `userMessageFromPrimary` ran for
    // both classified and unclassified branches. A classified error with
    // no per-entry message would silently pick up a status-derived canned
    // string — e.g. an HTTP 401 carrying `extensions.code: FORBIDDEN`
    // would surface "Run 'godaddy auth login'" as the userMessage of a
    // ServerError { kind: "FORBIDDEN" }, contradicting its own class.
    // The mapper now only consults HTTP status on the NetworkError path.
    withValidAuth();

    mockGraphQLErrorResponse(
      { extensions: { code: "FORBIDDEN" } }, // no `message`
      401, // status that would otherwise canned-string to "Run godaddy auth login"
    );

    const exit = await runEffectExit(
      getApplicationEffect("x", { accessToken: "test-token-123" }),
    );
    const err = extractFailure(exit) as ServerError;
    expect(err._tag).toBe("ServerError");
    expect(err.kind).toBe("FORBIDDEN");
    expect(err.userMessage).toBe("An unexpected error occurred");
    expect(err.userMessage).not.toContain("godaddy auth login");
    expect(err.userMessage).not.toContain("Access denied");

    // The envelope's `fix` (not `userMessage`) is what carries the
    // classification-appropriate guidance.
    const envelope = mapRuntimeError(err);
    expect(envelope.code).toBe("FORBIDDEN");
    expect(envelope.fix).toMatch(/permission/i);
  });

  test("NetworkError fallback still uses HTTP-status canned strings when no primary message", async () => {
    // The flip side of the regression above: status-based canned strings
    // remain useful on the unclassified path, where there's no
    // classification to contradict.
    withValidAuth();

    mockGraphQLErrorResponse(
      [{ extensions: { code: "OAUTH_CLIENT_UPSTREAM_ERROR" } }], // unknown code, no message
      503,
    );

    const exit = await runEffectExit(
      getApplicationEffect("x", { accessToken: "test-token-123" }),
    );
    const err = extractFailure(exit) as NetworkError;
    expect(err._tag).toBe("NetworkError");
    expect(err.userMessage).toBe(
      "The server encountered an error. Please try again later.",
    );
  });

  test("unclassified server code produces GraphQL-aware fix string in envelope", async () => {
    // End-to-end check: the NetworkError fallback branch must carry a
    // responseBody shape that the agent-side `hasGraphqlErrors` detector
    // recognizes (`{ errors: [...] }`), so agents get the tailored GraphQL
    // remediation hint rather than the generic connectivity fallback.
    withValidAuth();

    mockGraphQLErrorResponse({
      message: "Upstream OAuth client failure",
      extensions: { code: "OAUTH_CLIENT_UPSTREAM_ERROR" },
    });

    const exit = await runEffectExit(
      getApplicationEffect("x", { accessToken: "test-token-123" }),
    );
    const err = extractFailure(exit);
    const mapped = mapRuntimeError(err);
    expect(mapped.code).toBe("NETWORK_ERROR");
    expect(mapped.fix).toContain("error.details.response.errors");
    expect(JSON.stringify(mapped.details?.response)).toContain(
      "OAUTH_CLIENT_UPSTREAM_ERROR",
    );
  });

  test("transport failure (no ClientError) → NetworkError without HTTP context", async () => {
    withValidAuth();

    server.use(
      graphql.operation(() => {
        return HttpResponse.error();
      }),
    );

    const exit = await runEffectExit(
      getApplicationEffect("x", { accessToken: "test-token-123" }),
    );
    const err = extractFailure(exit) as NetworkError;
    expect(err._tag).toBe("NetworkError");
    // No HTTP context is available for raw transport failures.
    expect(err.status).toBeUndefined();
    expect(err.responseBody).toBeUndefined();
  });
});
