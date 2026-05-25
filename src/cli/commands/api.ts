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
import { truncateList, writeFullOutput } from "../agent/truncation";
import type { NextAction } from "../agent/types";
import {
  type CatalogDomain,
  type CatalogEndpoint,
  findEndpointByAnyMethodEffect,
  findEndpointByPathEffect,
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

const MAX_GRAPHQL_OPERATION_PREVIEW = 20;

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
        description: "API path (e.g. /location/addresses)",
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
          "Relative API endpoint path (e.g. /v1/commerce/location/addresses)",
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
      description: "Relative API endpoint path",
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
          description: "API path",
          value: next.path,
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

function searchNextActions(firstPath?: string): NextAction[] {
  const actions: NextAction[] = [];
  if (firstPath) {
    actions.push({
      command: "godaddy api describe <endpoint>",
      description: "Describe this endpoint",
      params: {
        endpoint: {
          description: "API path",
          value: firstPath,
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
          description: "Relative API endpoint path (e.g. /v1/domains)",
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

function isHttpMethod(value: string): value is HttpMethod {
  return VALID_METHODS.includes(value as HttpMethod);
}

const ABSOLUTE_HTTP_URL_PATTERN = /^https?:\/\//i;
const TRUSTED_API_HOSTS = new Set(["api.godaddy.com", "api.ote-godaddy.com"]);

interface ParsedEndpointInput {
  callEndpoint: string;
  catalogPathCandidates: string[];
  absoluteUrl: URL | null;
  isTrustedAbsolute: boolean;
  invalidAbsoluteUrl: boolean;
}

function parseAbsoluteHttpUrl(value: string): URL | null {
  try {
    const parsed = new URL(value.trim());
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
      return null;
    }
    return parsed;
  } catch {
    return null;
  }
}

function normalizeCatalogPath(pathValue: string): string {
  const pathOnly = pathValue.split(/[?#]/, 1)[0] || "/";
  const withLeadingSlash = pathOnly.startsWith("/") ? pathOnly : `/${pathOnly}`;

  if (withLeadingSlash.length > 1 && withLeadingSlash.endsWith("/")) {
    return withLeadingSlash.slice(0, -1);
  }

  return withLeadingSlash;
}

function buildCatalogPathCandidates(pathValue: string): string[] {
  const normalizedPath = normalizeCatalogPath(pathValue);
  const candidates = [normalizedPath];

  const commercePrefixMatch = normalizedPath.match(/^\/v\d+\/commerce(\/.*)$/i);
  if (commercePrefixMatch?.[1]) {
    candidates.push(commercePrefixMatch[1]);
  }

  return [...new Set(candidates)];
}

function parseEndpointInput(endpoint: string): ParsedEndpointInput {
  const trimmed = endpoint.trim();

  if (trimmed.length === 0) {
    return {
      callEndpoint: "/",
      catalogPathCandidates: ["/"],
      absoluteUrl: null,
      isTrustedAbsolute: false,
      invalidAbsoluteUrl: false,
    };
  }

  if (ABSOLUTE_HTTP_URL_PATTERN.test(trimmed)) {
    const absoluteUrl = parseAbsoluteHttpUrl(trimmed);
    if (!absoluteUrl) {
      return {
        callEndpoint: trimmed,
        catalogPathCandidates: buildCatalogPathCandidates(trimmed),
        absoluteUrl: null,
        isTrustedAbsolute: false,
        invalidAbsoluteUrl: true,
      };
    }

    const isTrustedAbsolute =
      absoluteUrl.protocol === "https:" &&
      TRUSTED_API_HOSTS.has(absoluteUrl.hostname.toLowerCase());

    const relativePath = `${absoluteUrl.pathname || "/"}${absoluteUrl.search}${absoluteUrl.hash}`;

    return {
      callEndpoint: isTrustedAbsolute ? relativePath : trimmed,
      catalogPathCandidates: buildCatalogPathCandidates(
        absoluteUrl.pathname || "/",
      ),
      absoluteUrl,
      isTrustedAbsolute,
      invalidAbsoluteUrl: false,
    };
  }

  const callEndpoint = trimmed.startsWith("/") ? trimmed : `/${trimmed}`;
  return {
    callEndpoint,
    catalogPathCandidates: buildCatalogPathCandidates(callEndpoint),
    absoluteUrl: null,
    isTrustedAbsolute: false,
    invalidAbsoluteUrl: false,
  };
}

function resolveCatalogEndpointEffect(
  method: HttpMethod,
  methodProvided: boolean,
  pathCandidates: string[],
  fallbackEndpoint: string,
): Effect.Effect<{
  method: HttpMethod;
  endpoint: string;
  catalogMatch?: { domain: CatalogDomain; endpoint: CatalogEndpoint };
}> {
  return Effect.gen(function* () {
    let catalogMatch:
      | { domain: CatalogDomain; endpoint: CatalogEndpoint }
      | undefined;
    let matchedPath: string | undefined;

    if (methodProvided) {
      for (const candidatePath of pathCandidates) {
        const byPath = yield* findEndpointByPathEffect(method, candidatePath);
        if (Option.isSome(byPath)) {
          catalogMatch = byPath.value;
          matchedPath = candidatePath;
          break;
        }
      }
    } else {
      for (const candidatePath of pathCandidates) {
        const byAnyMethod = yield* findEndpointByAnyMethodEffect(candidatePath);
        if (Option.isSome(byAnyMethod)) {
          catalogMatch = byAnyMethod.value;
          matchedPath = candidatePath;
          break;
        }
      }
    }

    let resolvedMethod = method;
    let resolvedEndpoint = fallbackEndpoint;

    if (catalogMatch) {
      if (matchedPath === catalogMatch.endpoint.path) {
        resolvedEndpoint = buildCallEndpoint(
          catalogMatch.domain,
          catalogMatch.endpoint,
        );
      }

      if (!methodProvided && isHttpMethod(catalogMatch.endpoint.method)) {
        resolvedMethod = catalogMatch.endpoint.method;
      }
    }

    return {
      method: resolvedMethod,
      endpoint: resolvedEndpoint,
      catalogMatch,
    };
  });
}

function buildCallEndpoint(
  domain: CatalogDomain,
  endpoint: CatalogEndpoint,
): string {
  return `${domain.baseUrl}${endpoint.path}`.replace(
    /^https:\/\/api\.godaddy\.com/i,
    "",
  );
}

function summarizeGraphqlSchema(graphql: CatalogEndpoint["graphql"]) {
  if (!graphql) return undefined;

  const queryCount = graphql.operations.filter(
    (operation) => operation.kind === "query",
  ).length;
  const mutationCount = graphql.operations.length - queryCount;

  const operationSummaries = graphql.operations.map((operation) => ({
    name: operation.name,
    kind: operation.kind,
    returnType: operation.returnType,
    deprecated: operation.deprecated,
    deprecationReason: operation.deprecationReason,
    args: operation.args.map((arg) => ({
      name: arg.name,
      type: arg.type,
      required: arg.required,
      defaultValue: arg.defaultValue,
    })),
  }));

  const shownOperations = operationSummaries.slice(
    0,
    MAX_GRAPHQL_OPERATION_PREVIEW,
  );

  return {
    schemaRef: graphql.schemaRef,
    operationCount: graphql.operationCount,
    queryCount,
    mutationCount,
    operations: shownOperations,
    operationsShown: shownOperations.length,
    operationsTruncated: operationSummaries.length > shownOperations.length,
  };
}

// ---------------------------------------------------------------------------
// Schema summarization for agent-friendly output
// ---------------------------------------------------------------------------

interface SchemaSummaryProperty {
  name: string;
  type: string;
  required: boolean;
  description?: string;
  format?: string;
  enum?: unknown[];
  items?: string;
}

/**
 * Summarize a JSON Schema into a compact representation suitable for LLM
 * context windows. Shows top-level properties with types, required markers,
 * and up to one level of nested object properties.
 */
function summarizeSchema(
  schema: Record<string, unknown> | undefined,
): SchemaSummaryProperty[] | undefined {
  if (!schema) return undefined;

  const properties = schema.properties as
    | Record<string, Record<string, unknown>>
    | undefined;
  if (!properties) {
    if (schema.type || schema.enum) {
      return [
        {
          name: "(value)",
          type: schemaTypeLabel(schema),
          required: true,
        },
      ];
    }
    return undefined;
  }

  const required = new Set(
    Array.isArray(schema.required) ? (schema.required as string[]) : [],
  );

  return Object.entries(properties).map(([name, prop]) => {
    const entry: SchemaSummaryProperty = {
      name,
      type: schemaTypeLabel(prop),
      required: required.has(name),
    };

    if (typeof prop.description === "string" && prop.description.length > 0) {
      entry.description = prop.description.slice(0, 120);
    }
    if (typeof prop.format === "string") {
      entry.format = prop.format;
    }
    if (Array.isArray(prop.enum) && prop.enum.length <= 10) {
      entry.enum = prop.enum;
    }
    if (
      prop.type === "array" &&
      typeof prop.items === "object" &&
      prop.items !== null
    ) {
      const items = prop.items as Record<string, unknown>;
      entry.items = schemaTypeLabel(items);
    }

    return entry;
  });
}

function schemaTypeLabel(schema: Record<string, unknown>): string {
  if (!schema) return "unknown";
  if (typeof schema.type === "string") {
    if (schema.type === "array" && schema.items) {
      const items = schema.items as Record<string, unknown>;
      return `array<${schemaTypeLabel(items)}>`;
    }
    if (schema.type === "object" && schema.properties) {
      const props = Object.keys(
        schema.properties as Record<string, unknown>,
      ).slice(0, 5);
      const more = Object.keys(schema.properties as object).length;
      const suffix = props.length < more ? ", ..." : "";
      return `object{${props.join(", ")}${suffix}}`;
    }
    return schema.format
      ? `${schema.type}(${schema.format})`
      : (schema.type as string);
  }
  if (Array.isArray(schema.enum)) {
    const vals = (schema.enum as unknown[]).slice(0, 8);
    const suffix = (schema.enum as unknown[]).length > 8 ? ", ..." : "";
    return `enum(${vals.join("|")}${suffix})`;
  }
  if (Array.isArray(schema.oneOf)) return "oneOf";
  if (Array.isArray(schema.anyOf)) return "anyOf";
  if (Array.isArray(schema.allOf)) return "allOf";
  if (schema.$ref) return `ref(${schema.$ref as string})`;
  return "object";
}

function summarizeResponses(
  responses: CatalogEndpoint["responses"] | undefined,
):
  | Record<string, { description: string; schema?: SchemaSummaryProperty[] }>
  | undefined {
  if (!responses) return undefined;
  const result: Record<
    string,
    { description: string; schema?: SchemaSummaryProperty[] }
  > = {};
  for (const [status, resp] of Object.entries(responses)) {
    result[status] = {
      description: resp.description || "",
      schema: summarizeSchema(
        resp.schema as Record<string, unknown> | undefined,
      ),
    };
  }
  return result;
}

function summarizeRequestBody(
  requestBody: CatalogEndpoint["requestBody"] | undefined,
):
  | {
      required: boolean;
      contentType?: string;
      description?: string;
      schema?: SchemaSummaryProperty[];
    }
  | undefined {
  if (!requestBody) return undefined;
  return {
    required: requestBody.required,
    contentType: requestBody.contentType,
    description: requestBody.description,
    schema: summarizeSchema(
      requestBody.schema as Record<string, unknown> | undefined,
    ),
  };
}

/**
 * Returns true if the summarized schemas contain nested object/array types
 * that the inline summary cannot fully represent.
 */
function hasComplexNestedTypes(
  requestSchema: SchemaSummaryProperty[] | undefined,
  responses:
    | Record<string, { description: string; schema?: SchemaSummaryProperty[] }>
    | undefined,
): boolean {
  const check = (props: SchemaSummaryProperty[] | undefined) =>
    props?.some(
      (p) => p.type.startsWith("object{") || p.type.startsWith("array<object"),
    ) ?? false;

  if (check(requestSchema)) return true;
  if (responses) {
    for (const resp of Object.values(responses)) {
      if (check(resp.schema)) return true;
    }
  }
  return false;
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
          graphql_operations: e.graphql?.operationCount,
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
                      description: "API path",
                      value: endpointSummaries[0].path,
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
      Args.withDescription("API path (e.g. /location/addresses)"),
    ),
    method: Options.text("method").pipe(
      Options.withAlias("m"),
      Options.withDescription("HTTP method (GET, POST, PUT, PATCH, DELETE)"),
      Options.optional,
    ),
  },
  ({ endpoint, method }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;

      const { catalogPathCandidates: pathCandidates } =
        parseEndpointInput(endpoint);

      const methodFilter = Option.getOrUndefined(method)?.toUpperCase();

      // Try exact path lookup first
      let result: Option.Option<{
        domain: CatalogDomain;
        endpoint: CatalogEndpoint;
      }> = Option.none();

      for (const candidatePath of pathCandidates) {
        if (methodFilter) {
          const exactMatch = yield* findEndpointByPathEffect(
            methodFilter,
            candidatePath,
          );
          if (Option.isSome(exactMatch)) {
            result = exactMatch;
            break;
          }
        } else {
          const exactMatch =
            yield* findEndpointByAnyMethodEffect(candidatePath);
          if (Option.isSome(exactMatch)) {
            result = exactMatch;
            break;
          }
        }
      }

      // Fallback: fuzzy search
      if (Option.isNone(result)) {
        const searchResults = yield* searchEndpointsEffect(pathCandidates[0]);

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
                  description: "API path",
                  value: m.path,
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

      const summarizedRequest = summarizeRequestBody(ep.requestBody);
      const summarizedResponses = summarizeResponses(ep.responses);

      // Only write the full schema file + hint when there are nested
      // objects that the summary can't fully represent.
      const hasNestedObjects = hasComplexNestedTypes(
        summarizedRequest?.schema,
        summarizedResponses,
      );

      let schemaDetail: { note: string; file: string } | undefined;
      if (hasNestedObjects) {
        const fullSchemaPath = writeFullOutput(
          `api-describe-${ep.operationId}`,
          {
            domain: domain.name,
            baseUrl: domain.baseUrl,
            operationId: ep.operationId,
            method: ep.method,
            path: ep.path,
            parameters: ep.parameters,
            requestBody: ep.requestBody,
            responses: ep.responses,
            scopes: ep.scopes,
          },
        );
        schemaDetail = {
          note: "Inline schemas show top-level property names and types. Read the file below for full nested object definitions.",
          file: fullSchemaPath,
        };
      }

      yield* writer.emitSuccess(
        "godaddy api describe",
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
          requestBody: summarizedRequest,
          responses: summarizedResponses,
          scopes: ep.scopes,
          graphql: summarizeGraphqlSchema(ep.graphql),
          schema_detail: schemaDetail,
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
        "Search term (matches path, summary, and description)",
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
        graphql_operations: r.endpoint.graphql?.operationCount,
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
        searchNextActions(items[0]?.path),
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
        "API endpoint path (for example: /v1/commerce/location/addresses)",
      ),
    ),
    method: Options.text("method").pipe(
      Options.withAlias("X"),
      Options.withDescription("HTTP method (GET, POST, PUT, PATCH, DELETE)"),
      Options.optional,
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

      const methodInput = Option.getOrElse(
        config.method,
        () => "GET",
      ).toUpperCase();
      if (!isHttpMethod(methodInput)) {
        return yield* Effect.fail(
          new ValidationError({
            message: `Invalid HTTP method: ${methodInput}`,
            userMessage: `Method must be one of: ${VALID_METHODS.join(", ")}`,
          }),
        );
      }

      const methodProvided = Option.isSome(config.method);
      const parsedEndpoint = parseEndpointInput(config.endpoint);

      if (parsedEndpoint.invalidAbsoluteUrl) {
        return yield* Effect.fail(
          new ValidationError({
            message: `Invalid endpoint URL: ${config.endpoint}`,
            userMessage:
              "Endpoint must be a valid URL or a relative path (for example: /v1/domains).",
          }),
        );
      }

      if (parsedEndpoint.absoluteUrl && !parsedEndpoint.isTrustedAbsolute) {
        return yield* Effect.fail(
          new ValidationError({
            message: `Untrusted endpoint host: ${parsedEndpoint.absoluteUrl.hostname}`,
            userMessage:
              "Use a relative endpoint path, or a trusted GoDaddy API URL on api.godaddy.com or api.ote-godaddy.com.",
          }),
        );
      }

      const resolved = yield* resolveCatalogEndpointEffect(
        methodInput,
        methodProvided,
        parsedEndpoint.catalogPathCandidates,
        parsedEndpoint.callEndpoint,
      );

      const method = resolved.method;
      const resolvedEndpoint = resolved.endpoint;
      const catalogMatch = resolved.catalogMatch;

      if (catalogMatch && cliConfig.verbosity >= 1) {
        process.stderr.write(
          `Resolved endpoint to ${catalogMatch.endpoint.method} ${resolvedEndpoint}\n`,
        );
      }

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

      const requiredScopesSet = new Set(
        config.scope.flatMap((scopeToken) =>
          scopeToken
            .split(/[\s,]+/)
            .map((token) => token.trim())
            .filter((token) => token.length > 0),
        ),
      );

      if (catalogMatch) {
        for (const scope of catalogMatch.endpoint.scopes) {
          requiredScopesSet.add(scope);
        }
      }

      const requiredScopes = [...requiredScopesSet];

      const graphqlRequest =
        catalogMatch?.endpoint.graphql !== undefined ||
        /\/graphql(?:$|[/?#])/i.test(resolvedEndpoint);

      const requestOpts = {
        endpoint: resolvedEndpoint,
        method,
        fields: Object.keys(fields).length > 0 ? fields : undefined,
        body,
        headers: Object.keys(headers).length > 0 ? headers : undefined,
        debug: cliConfig.verbosity >= 2,
        graphql: graphqlRequest,
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
          endpoint: resolvedEndpoint,
          method,
          status: response.status,
          status_text: response.statusText,
          resolved:
            catalogMatch === undefined
              ? undefined
              : {
                  domain: catalogMatch.domain.name,
                  path: catalogMatch.endpoint.path,
                  method: catalogMatch.endpoint.method,
                  scopes: catalogMatch.endpoint.scopes,
                  graphql_operations:
                    catalogMatch.endpoint.graphql?.operationCount,
                },
          scopes_requested:
            requiredScopes.length > 0 ? requiredScopes : undefined,
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
              "Show detailed schema information for an API endpoint (by path)",
            usage: "godaddy api describe <path>",
          },
          {
            command: "godaddy api search <query>",
            description: "Search for API endpoints by keyword",
            usage: "godaddy api search <query>",
          },
          {
            command: "godaddy api call <endpoint>",
            description: "Make an authenticated API request (endpoint path)",
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
