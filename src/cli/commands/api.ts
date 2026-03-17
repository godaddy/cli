import * as Args from "@effect/cli/Args";
import * as Command from "@effect/cli/Command";
import * as Options from "@effect/cli/Options";
import * as Effect from "effect/Effect";
import * as Option from "effect/Option";
import {
  type HttpMethod,
  apiRequestEffect,
  parseFieldsEffect,
  parseHeadersEffect,
  readBodyFromFileEffect,
  sanitizeResponseHeaders,
} from "../../core/api";
import { authLoginEffect, getTokenInfoEffect } from "../../core/auth";
import { AuthenticationError, ValidationError } from "../../effect/errors";
import { protectPayload, truncateList } from "../agent/truncation";
import type { NextAction } from "../agent/types";
import {
  type CatalogDomain,
  type CatalogEndpoint,
  findEndpointByAnyMethodEffect,
  findEndpointByOperationIdEffect,
  listDomainsEffect,
  loadDomainEffect,
  searchEndpointsEffect,
} from "../schemas/api/index";
import { CliConfig } from "../services/cli-config";
import { EnvelopeWriter } from "../services/envelope-writer";
const VALID_METHODS: readonly HttpMethod[] = [
  "GET",
  "POST",
  "PUT",
  "PATCH",
  "DELETE",
];

// ---------------------------------------------------------------------------
// next_actions helpers
// ---------------------------------------------------------------------------

const apiGroupActions: NextAction[] = [
  {
    command: "godaddy api list",
    description: "List all API domains and endpoints",
  },
  {
    command: "godaddy api describe <endpoint>",
    description: "Describe an API endpoint's schema and parameters",
    params: {
      endpoint: {
        description:
          "Operation ID or path (e.g. commerce.location.verify-address or /location/addresses)",
        required: true,
      },
    },
  },
  {
    command: "godaddy api search <query>",
    description: "Search API endpoints by keyword",
    params: {
      query: { description: "Search term", required: true },
    },
  },
  {
    command: "godaddy api call <endpoint>",
    description: "Make an authenticated API request",
    params: {
      endpoint: {
        description:
          "Relative API endpoint (e.g. /v1/commerce/location/addresses)",
        required: true,
      },
    },
  },
];

function describeNextActions(
  domain: CatalogDomain,
  endpoint: CatalogEndpoint,
): NextAction[] {
  // Build a call template with strongly-typed params instead of embedding
  // schema-sourced values directly into an executable command string.
  const fullPath = `${domain.baseUrl}${endpoint.path}`.replace(
    /^https:\/\/api\.godaddy\.com/,
    "",
  );
  const callParams: NonNullable<NextAction["params"]> = {
    endpoint: {
      description: "Relative API endpoint",
      value: fullPath,
      required: true,
    },
    method: {
      description: "HTTP method",
      value: endpoint.method,
    },
  };
  if (endpoint.scopes.length > 0) {
    callParams.scope = {
      description: "Required OAuth scope",
      value: endpoint.scopes[0],
    };
  }

  const actions: NextAction[] = [
    {
      command: "godaddy api call <endpoint>",
      description: `Execute ${endpoint.method} ${endpoint.path}`,
      params: callParams,
    },
    {
      command: "godaddy api list",
      description: "List all API domains and endpoints",
    },
  ];

  // Suggest other endpoints in the same domain
  const otherEndpoints = domain.endpoints.filter(
    (e) => e.operationId !== endpoint.operationId,
  );
  if (otherEndpoints.length > 0) {
    const next = otherEndpoints[0];
    actions.push({
      command: "godaddy api describe <endpoint>",
      description: `Describe ${next.summary}`,
      params: {
        endpoint: {
          description: "Operation ID or path",
          value: next.operationId,
          required: true,
        },
      },
    });
  }

  return actions;
}

function listNextActions(firstDomain?: string): NextAction[] {
  return [
    {
      command: "godaddy api list --domain <domain>",
      description: "List endpoints for a specific API domain",
      params: {
        domain: {
          description: "Domain name",
          value: firstDomain,
          required: true,
        },
      },
    },
    {
      command: "godaddy api search <query>",
      description: "Search for API endpoints by keyword",
      params: {
        query: { description: "Search term", required: true },
      },
    },
  ];
}

function searchNextActions(firstOperationId?: string): NextAction[] {
  const actions: NextAction[] = [];
  if (firstOperationId) {
    actions.push({
      command: "godaddy api describe <endpoint>",
      description: "Describe this endpoint",
      params: {
        endpoint: {
          description: "Operation ID or path",
          value: firstOperationId,
          required: true,
        },
      },
    });
  }
  actions.push({
    command: "godaddy api list",
    description: "List all API domains",
  });
  return actions;
}

function callNextActions(): NextAction[] {
  return [
    {
      command: "godaddy api call <endpoint>",
      description: "Call another API endpoint",
      params: {
        endpoint: {
          description: "Relative API endpoint (e.g. /v1/domains)",
          required: true,
        },
      },
    },
    { command: "godaddy auth status", description: "Check auth status" },
    {
      command: "godaddy api list",
      description: "Browse available API endpoints",
    },
  ];
}

// ---------------------------------------------------------------------------
// extractPath — public for unit testing
// ---------------------------------------------------------------------------

/**
 * Extract a value from an object using a simple JSON path.
 * Supports: .key, .key.nested, .key[0], .key[0].nested
 */
export function extractPath(obj: unknown, path: string): unknown {
  if (!path || path === ".") {
    return obj;
  }

  const normalizedPath = path.startsWith(".") ? path.slice(1) : path;
  if (!normalizedPath) {
    return obj;
  }

  const segments: Array<string | number> = [];
  const regex = /([\w-]+)|\[(\d+)\]/g;
  for (const match of normalizedPath.matchAll(regex)) {
    const key = match[1];
    const index = match[2];
    if (key !== undefined) {
      segments.push(key);
    } else if (index !== undefined) {
      segments.push(Number.parseInt(index, 10));
    }
  }

  let current: unknown = obj;
  for (const segment of segments) {
    if (current === null || current === undefined) {
      return undefined;
    }

    if (typeof segment === "number") {
      if (!Array.isArray(current)) {
        throw new Error(`Cannot index non-array with [${segment}]`);
      }
      current = current[segment];
      continue;
    }

    if (typeof current !== "object") {
      throw new Error(`Cannot access property "${segment}" on non-object`);
    }

    current = (current as Record<string, unknown>)[segment];
  }

  return current;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function normalizeStringArray(value: ReadonlyArray<string>): string[] {
  return value.filter((entry): entry is string => typeof entry === "string");
}

/** Decode a JWT payload without verification (we only need the claims). */
function decodeJwtPayload(token: string): Record<string, unknown> | null {
  try {
    const parts = token.split(".");
    if (parts.length !== 3) return null;
    const payload = Buffer.from(parts[1], "base64url").toString("utf-8");
    return JSON.parse(payload) as Record<string, unknown>;
  } catch {
    return null;
  }
}

/** Check whether a JWT already contains every scope in `required`. */
function tokenHasScopes(token: string, required: string[]): boolean {
  if (required.length === 0) return true;
  const claims = decodeJwtPayload(token);
  if (!claims || typeof claims.scope !== "string") return false;
  const granted = new Set(claims.scope.split(/\s+/));
  return required.every((s) => granted.has(s));
}

// ---------------------------------------------------------------------------
// Subcommand: api list
// ---------------------------------------------------------------------------

const apiList = Command.make(
  "list",
  {
    domain: Options.text("domain").pipe(
      Options.withAlias("d"),
      Options.withDescription("Filter by API domain name"),
      Options.optional,
    ),
  },
  (config) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const domainFilter = Option.getOrUndefined(config.domain);

      if (domainFilter) {
        // List endpoints for a specific domain
        const maybeDomain = yield* loadDomainEffect(domainFilter);
        if (Option.isNone(maybeDomain)) {
          return yield* Effect.fail(
            new ValidationError({
              message: `API domain '${domainFilter}' not found`,
              userMessage: `API domain '${domainFilter}' does not exist. Run: godaddy api list`,
            }),
          );
        }
        const domain = maybeDomain.value;

        const endpointSummaries = domain.endpoints.map((e) => ({
          operationId: e.operationId,
          method: e.method,
          path: e.path,
          summary: e.summary,
          scopes: e.scopes,
        }));

        const truncated = truncateList(
          endpointSummaries,
          `api-list-${domainFilter}`,
        );

        yield* writer.emitSuccess(
          "godaddy api list",
          {
            domain: domain.name,
            title: domain.title,
            description: domain.description,
            version: domain.version,
            baseUrl: domain.baseUrl,
            endpoints: truncated.items,
            total: truncated.metadata.total,
            shown: truncated.metadata.shown,
            truncated: truncated.metadata.truncated,
            full_output: truncated.metadata.full_output,
          },
          endpointSummaries.length > 0
            ? [
                {
                  command: "godaddy api describe <endpoint>",
                  description: `Describe ${endpointSummaries[0].summary}`,
                  params: {
                    endpoint: {
                      description: "Operation ID or path",
                      value: endpointSummaries[0].operationId,
                      required: true,
                    },
                  },
                },
                {
                  command: "godaddy api list",
                  description: "List all API domains",
                },
                {
                  command: "godaddy api search <query>",
                  description: "Search for endpoints by keyword",
                  params: {
                    query: { description: "Search term", required: true },
                  },
                },
              ]
            : listNextActions(),
        );
      } else {
        // List all domains
        const domains = yield* listDomainsEffect();
        const truncated = truncateList(domains, "api-list-domains");

        yield* writer.emitSuccess(
          "godaddy api list",
          {
            domains: truncated.items,
            total: truncated.metadata.total,
            shown: truncated.metadata.shown,
            truncated: truncated.metadata.truncated,
            full_output: truncated.metadata.full_output,
          },
          listNextActions(domains[0]?.name),
        );
      }
    }),
).pipe(Command.withDescription("List available API domains and endpoints"));

// ---------------------------------------------------------------------------
// Subcommand: api describe
// ---------------------------------------------------------------------------

const apiDescribe = Command.make(
  "describe",
  {
    endpoint: Args.text({ name: "endpoint" }).pipe(
      Args.withDescription(
        "Operation ID (e.g. commerce.location.verify-address) or path (e.g. /location/addresses)",
      ),
    ),
  },
  ({ endpoint }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;

      // Try to find by operation ID first, then by path
      let result = yield* findEndpointByOperationIdEffect(endpoint);

      if (Option.isNone(result)) {
        // Try as a path, testing all HTTP methods
        result = yield* findEndpointByAnyMethodEffect(endpoint);
      }

      // Fallback: fuzzy search
      if (Option.isNone(result)) {
        const searchResults = yield* searchEndpointsEffect(endpoint);

        if (searchResults.length === 1) {
          result = Option.some(searchResults[0]);
        } else if (searchResults.length > 1) {
          // Multiple matches — list them for the agent to choose
          const matches = searchResults.map((r) => ({
            operationId: r.endpoint.operationId,
            method: r.endpoint.method,
            path: r.endpoint.path,
            summary: r.endpoint.summary,
            domain: r.domain.name,
          }));
          yield* writer.emitSuccess(
            "godaddy api describe",
            {
              message: `Multiple endpoints match '${endpoint}'. Be more specific:`,
              matches,
            },
            matches.map((m) => ({
              command: "godaddy api describe <endpoint>",
              description: `${m.method} ${m.path} — ${m.summary}`,
              params: {
                endpoint: {
                  description: "Operation ID or path",
                  value: m.operationId,
                  required: true,
                },
              },
            })),
          );
          return;
        }
      }

      if (Option.isNone(result)) {
        return yield* Effect.fail(
          new ValidationError({
            message: `Endpoint '${endpoint}' not found`,
            userMessage: `Endpoint '${endpoint}' not found in the API catalog. Run: godaddy api list or godaddy api search <query>`,
          }),
        );
      }

      const { domain, endpoint: ep } = result.value;

      const payload = protectPayload(
        {
          domain: domain.name,
          baseUrl: domain.baseUrl,
          operationId: ep.operationId,
          method: ep.method,
          path: ep.path,
          fullPath: `${domain.baseUrl}${ep.path}`.replace(
            /^https:\/\/api\.godaddy\.com/,
            "",
          ),
          summary: ep.summary,
          description: ep.description,
          parameters: ep.parameters,
          requestBody: ep.requestBody,
          responses: ep.responses,
          scopes: ep.scopes,
        },
        `api-describe-${ep.operationId}`,
      );

      yield* writer.emitSuccess(
        "godaddy api describe",
        {
          ...payload.value,
          truncated: payload.metadata?.truncated ?? false,
          total: payload.metadata?.total,
          shown: payload.metadata?.shown,
          full_output: payload.metadata?.full_output,
        },
        describeNextActions(domain, ep),
      );
    }),
).pipe(
  Command.withDescription(
    "Show detailed schema information for an API endpoint",
  ),
);

// ---------------------------------------------------------------------------
// Subcommand: api search
// ---------------------------------------------------------------------------

const apiSearch = Command.make(
  "search",
  {
    query: Args.text({ name: "query" }).pipe(
      Args.withDescription(
        "Search term (matches operation ID, summary, description, path)",
      ),
    ),
  },
  ({ query }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const results = yield* searchEndpointsEffect(query);

      const items = results.map((r) => ({
        operationId: r.endpoint.operationId,
        method: r.endpoint.method,
        path: r.endpoint.path,
        summary: r.endpoint.summary,
        domain: r.domain.name,
        scopes: r.endpoint.scopes,
      }));

      const truncated = truncateList(items, `api-search-${query}`);

      yield* writer.emitSuccess(
        "godaddy api search",
        {
          query,
          results: truncated.items,
          total: truncated.metadata.total,
          shown: truncated.metadata.shown,
          truncated: truncated.metadata.truncated,
          full_output: truncated.metadata.full_output,
        },
        searchNextActions(items[0]?.operationId),
      );
    }),
).pipe(Command.withDescription("Search for API endpoints by keyword"));

// ---------------------------------------------------------------------------
// Subcommand: api call (the original raw request behavior)
// ---------------------------------------------------------------------------

const apiCall = Command.make(
  "call",
  {
    endpoint: Args.text({ name: "endpoint" }).pipe(
      Args.withDescription(
        "API endpoint (for example: /v1/commerce/location/addresses)",
      ),
    ),
    method: Options.text("method").pipe(
      Options.withAlias("X"),
      Options.withDescription("HTTP method (GET, POST, PUT, PATCH, DELETE)"),
      Options.withDefault("GET"),
    ),
    field: Options.text("field").pipe(
      Options.withAlias("f"),
      Options.withDescription("Add request body field (can be repeated)"),
      Options.repeated,
    ),
    file: Options.text("file").pipe(
      Options.withAlias("F"),
      Options.withDescription("Read request body from JSON file"),
      Options.optional,
    ),
    header: Options.text("header").pipe(
      Options.withAlias("H"),
      Options.withDescription("Add custom header (can be repeated)"),
      Options.repeated,
    ),
    query: Options.text("query").pipe(
      Options.withAlias("q"),
      Options.withDescription(
        "Extract a value from response JSON (for example: .data[0].id)",
      ),
      Options.optional,
    ),
    include: Options.boolean("include").pipe(
      Options.withAlias("i"),
      Options.withDescription("Include response headers in result"),
    ),
    scope: Options.text("scope").pipe(
      Options.withAlias("s"),
      Options.withDescription(
        "Required OAuth scope. On 403, triggers auth and retries (can be repeated)",
      ),
      Options.repeated,
    ),
  },
  (config) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const cliConfig = yield* CliConfig;
      const methodInput = config.method.toUpperCase();

      if (!VALID_METHODS.includes(methodInput as HttpMethod)) {
        return yield* Effect.fail(
          new ValidationError({
            message: `Invalid HTTP method: ${config.method}`,
            userMessage: `Method must be one of: ${VALID_METHODS.join(", ")}`,
          }),
        );
      }

      const method = methodInput as HttpMethod;
      const fields = yield* parseFieldsEffect(
        normalizeStringArray(config.field),
      );
      const headers = yield* parseHeadersEffect(
        normalizeStringArray(config.header),
      );

      let body: string | undefined;
      const filePath = Option.getOrUndefined(config.file);
      if (typeof filePath === "string" && filePath.length > 0) {
        body = yield* readBodyFromFileEffect(filePath);
      }

      const requiredScopes = config.scope.flatMap((s) =>
        s
          .split(/[\s,]+/)
          .map((t) => t.trim())
          .filter((t) => t.length > 0),
      );

      const requestOpts = {
        endpoint: config.endpoint,
        method,
        fields: Object.keys(fields).length > 0 ? fields : undefined,
        body,
        headers: Object.keys(headers).length > 0 ? headers : undefined,
        debug: cliConfig.verbosity >= 2,
      };

      // First attempt
      const response = yield* apiRequestEffect(requestOpts).pipe(
        Effect.catchAll((error) => {
          // On 403 with --scope: check if the token is missing the scope,
          // trigger auth, and retry — once.
          if (
            error._tag === "AuthenticationError" &&
            error.message.includes("403") &&
            requiredScopes.length > 0
          ) {
            return Effect.gen(function* () {
              // Get current token to inspect scopes
              const tokenInfo = yield* getTokenInfoEffect().pipe(
                Effect.catchAll(() => Effect.succeed(null)),
              );

              if (
                tokenInfo &&
                tokenHasScopes(tokenInfo.accessToken, requiredScopes)
              ) {
                // Token already has the scopes — the 403 is not a scope issue
                return yield* Effect.fail(error);
              }

              // Token is missing required scopes — re-auth and retry
              if (cliConfig.verbosity >= 1) {
                process.stderr.write(
                  `Token missing scope(s): ${requiredScopes.join(", ")}. Triggering auth flow...\n`,
                );
              }

              const loginResult = yield* authLoginEffect({
                additionalScopes: requiredScopes,
              }).pipe(
                Effect.catchAll(() =>
                  Effect.fail(
                    new AuthenticationError({
                      message: "Re-authentication failed",
                      userMessage:
                        "Automatic re-authentication failed. Run 'godaddy auth login' manually.",
                    }),
                  ),
                ),
              );

              if (!loginResult.success) {
                return yield* Effect.fail(
                  new AuthenticationError({
                    message: "Re-authentication did not succeed",
                    userMessage:
                      "Authentication did not complete. Run 'godaddy auth login' manually.",
                  }),
                );
              }

              // Retry the request with the new token
              return yield* apiRequestEffect(requestOpts);
            });
          }
          return Effect.fail(error);
        }),
      );

      let output = response.data;
      const queryPath = Option.getOrUndefined(config.query);
      if (typeof queryPath === "string" && output !== undefined) {
        try {
          output = extractPath(output, queryPath);
        } catch (error) {
          const message =
            error instanceof Error ? error.message : String(error);
          return yield* Effect.fail(
            new ValidationError({
              message: `Invalid query path: ${queryPath}`,
              userMessage: `Query error: ${message}`,
            }),
          );
        }
      }

      yield* writer.emitSuccess(
        "godaddy api call",
        {
          endpoint: config.endpoint.startsWith("/")
            ? config.endpoint
            : `/${config.endpoint}`,
          method,
          status: response.status,
          status_text: response.statusText,
          headers: config.include
            ? sanitizeResponseHeaders(response.headers)
            : undefined,
          data: output ?? null,
        },
        callNextActions(),
      );
    }),
).pipe(
  Command.withDescription("Make authenticated requests to the GoDaddy API"),
);

// ---------------------------------------------------------------------------
// Parent command: godaddy api
// ---------------------------------------------------------------------------

const apiParent = Command.make("api", {}, () =>
  Effect.gen(function* () {
    const writer = yield* EnvelopeWriter;

    const domains = yield* listDomainsEffect();

    yield* writer.emitSuccess(
      "godaddy api",
      {
        command: "godaddy api",
        description:
          "Explore and call GoDaddy API endpoints. Use subcommands to discover endpoints before making requests.",
        commands: [
          {
            command: "godaddy api list",
            description: "List all API domains and their endpoints",
            usage: "godaddy api list [--domain <domain>]",
          },
          {
            command: "godaddy api describe <endpoint>",
            description:
              "Show detailed schema information for an API endpoint (by operation ID or path)",
            usage: "godaddy api describe <operationId-or-path>",
          },
          {
            command: "godaddy api search <query>",
            description: "Search for API endpoints by keyword",
            usage: "godaddy api search <query>",
          },
          {
            command: "godaddy api call <endpoint>",
            description: "Make an authenticated API request",
            usage:
              "godaddy api call <endpoint> [-X method] [-f field=value] [-F file] [-H header] [-q path] [-i] [-s scope]",
          },
        ],
        domains: domains.map((d) => ({
          name: d.name,
          title: d.title,
          endpoints: d.endpointCount,
        })),
      },
      apiGroupActions,
    );
  }),
).pipe(
  Command.withDescription("Explore and call GoDaddy API endpoints"),
  Command.withSubcommands([apiList, apiDescribe, apiSearch, apiCall]),
);

export { apiParent as apiCommand };
