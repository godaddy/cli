/**
 * Build-time script: discovers OpenAPI specs from gdcorp-platform repositories
 * and produces a JSON catalog that the CLI bundles for:
 *   - godaddy api list
 *   - godaddy api describe
 *
 * Also resolves external $ref URLs (e.g. schemas.api.godaddy.com) at build time
 * so the CLI catalog is fully self-contained.
 *
 * Usage:
 *   pnpm tsx scripts/generate-api-catalog.ts
 *
 * Optional environment variables:
 *   GITHUB_TOKEN                GitHub token for higher API rate limits
 *   API_CATALOG_REPOS           Comma-separated repo names to include
 *                               (e.g. "commerce.catalog-products-specification,commerce.orders-specification")
 *   API_CATALOG_REPO_REFS       Optional comma-separated repo=gitRef overrides
 *                               (e.g. "commerce.catalog-products-specification=pull/81/head")
 *   API_CATALOG_INCLUDE_LEGACY_LOCATION
 *                               "false" to exclude location.addresses-specification
 *
 * Output:
 *   src/cli/schemas/api/manifest.json          – domain index
 *   src/cli/schemas/api/<domain>.json          – per-domain endpoint catalog
 *   src/cli/schemas/api/registry.generated.ts  – generated runtime registry
 */

import { execFileSync } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import $RefParser from "@apidevtools/json-schema-ref-parser";
import type { ParserOptions } from "@apidevtools/json-schema-ref-parser";
import {
  Kind,
  type TypeNode,
  parse as parseGraphql,
  print as printGraphql,
} from "graphql";
import { parse as parseYamlStrict } from "yaml";

/** Parse YAML with lenient settings (duplicate keys: last wins). */
function parseYaml(src: string): unknown {
  return parseYamlStrict(src, { uniqueKeys: false });
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface OpenApiParameter {
  name: string;
  in: string;
  required?: boolean;
  description?: string;
  schema?: Record<string, unknown>;
}

interface OpenApiReference {
  $ref: string;
}

interface OpenApiRequestBody {
  description?: string;
  required?: boolean;
  content?: Record<string, { schema?: Record<string, unknown> }>;
}

interface OpenApiResponse {
  description?: string;
  content?: Record<string, { schema?: Record<string, unknown> }>;
}

interface OpenApiOperation {
  operationId?: string;
  summary?: string;
  description?: string;
  parameters?: Array<OpenApiParameter | OpenApiReference>;
  requestBody?: OpenApiRequestBody | OpenApiReference;
  responses?: Record<string, OpenApiResponse | OpenApiReference>;
  security?: Array<Record<string, string[]>>;
  "x-godaddy-graphql-schema"?: string;
}

interface OpenApiPathItem {
  [method: string]:
    | OpenApiOperation
    | Array<OpenApiParameter | OpenApiReference>
    | undefined;
  parameters?: Array<OpenApiParameter | OpenApiReference>;
}

interface OpenApiServer {
  url: string;
  variables?: Record<string, { default: string; enum?: string[] }>;
}

interface OpenApiSpec {
  openapi: string;
  info: {
    title: string;
    description?: string;
    version: string;
    contact?: Record<string, string>;
  };
  paths: Record<string, OpenApiPathItem>;
  servers?: OpenApiServer[];
  components?: Record<string, unknown>;
}

// ---------------------------------------------------------------------------
// Output types — what the CLI consumes at runtime
// ---------------------------------------------------------------------------

interface CatalogGraphqlArgument {
  name: string;
  type: string;
  required: boolean;
  description?: string;
  defaultValue?: string;
}

interface CatalogGraphqlOperation {
  name: string;
  kind: "query" | "mutation";
  returnType: string;
  description?: string;
  deprecated: boolean;
  deprecationReason?: string;
  args: CatalogGraphqlArgument[];
}

interface CatalogGraphqlSchema {
  schemaRef: string;
  operationCount: number;
  operations: CatalogGraphqlOperation[];
}

interface CatalogEndpoint {
  operationId: string;
  method: string;
  path: string;
  summary: string;
  description?: string;
  parameters?: Array<{
    name: string;
    in: string;
    required: boolean;
    description?: string;
    schema?: Record<string, unknown>;
  }>;
  requestBody?: {
    required: boolean;
    description?: string;
    contentType: string;
    schema?: Record<string, unknown>;
  };
  responses: Record<
    string,
    {
      description: string;
      schema?: Record<string, unknown>;
    }
  >;
  scopes: string[];
  graphql?: CatalogGraphqlSchema;
}

interface CatalogDomain {
  name: string;
  title: string;
  description: string;
  version: string;
  baseUrl: string;
  endpoints: CatalogEndpoint[];
}

interface CatalogManifest {
  generated: string;
  domains: Record<
    string,
    {
      file: string;
      title: string;
      endpointCount: number;
    }
  >;
}

interface GitHubRepo {
  name: string;
  cloneUrl: string;
  archived: boolean;
  disabled: boolean;
  private: boolean;
}

interface SpecSource {
  domain: string;
  repoName: string;
  specFile: string;
  specVersion: string;
  graphqlOnly?: boolean;
}

interface DiscoveredSpecSources {
  sources: SpecSource[];
  cloneRoot: string | null;
}

// ---------------------------------------------------------------------------
// Discovery configuration
// ---------------------------------------------------------------------------

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PROJECT_ROOT = path.resolve(__dirname, "..");
const OUTPUT_DIR = path.resolve(PROJECT_ROOT, "src/cli/schemas/api");
const REGISTRY_FILE = path.join(OUTPUT_DIR, "registry.generated.ts");

const GITHUB_ORG = "gdcorp-platform";
const GITHUB_API_BASE = "https://api.github.com";
const GITHUB_REPOS_PAGE_SIZE = 100;
const COMMERCE_SPEC_REPO_PATTERN = /^commerce\.[a-z0-9-]+-specification$/;
const BOOTSTRAP_COMMERCE_REPOS = [
  "commerce.bulk-operations-specification",
  "commerce.businesses-specification",
  "commerce.catalog-products-specification",
  "commerce.channels-specification",
  "commerce.chargebacks-specification",
  "commerce.customer-profiles-specification",
  "commerce.fulfillments-specification",
  "commerce.metafields-specification",
  "commerce.onboarding-specification",
  "commerce.orders-specification",
  "commerce.payment-requests-specification",
  "commerce.payments-specification",
  "commerce.price-adjustments-specification",
  "commerce.recommendations-specification",
  "commerce.shipping-specification",
  "commerce.stores-specification",
  "commerce.subscriptions-specification",
  "commerce.taxes-specification",
  "commerce.transactions-specification",
];
const LEGACY_ALWAYS_INCLUDE_REPOS = ["location.addresses-specification"];

// ---------------------------------------------------------------------------
// $ref resolution via json-schema-ref-parser
// ---------------------------------------------------------------------------

/**
 * Path to the cloned common-types-specification repo, set during discovery.
 * Used by the custom resolver to map https://schemas.api.godaddy.com URLs
 * to local files.
 */
let commonTypesLocalDir: string | null = null;

/**
 * Dereference an OpenAPI spec file, resolving all $ref pointers in-place.
 * Uses json-schema-ref-parser with a custom resolver that maps
 * https://schemas.api.godaddy.com/common-types/... to local files.
 */
/**
 * Given a file path containing "common-types", try to find the actual file
 * in the local common-types clone. Handles multiple path patterns used across
 * repos:
 *   - ./common-types/v1/schemas/yaml/foo.yaml  (full nested path)
 *   - ./common-types/foo.json                   (flat shortcut)
 */
function resolveCommonTypesFile(filePath: string): string | null {
  if (!commonTypesLocalDir) return null;

  const basename = path.basename(filePath);
  const ext = path.extname(basename).toLowerCase();

  // Try the path as-is relative to common-types root
  // e.g. common-types/v1/schemas/yaml/foo.yaml
  const idx = filePath.indexOf("common-types");
  if (idx >= 0) {
    const relPath = filePath.slice(idx + "common-types/".length);
    const direct = path.join(commonTypesLocalDir, relPath);
    if (fs.existsSync(direct)) return direct;
  }

  // Flat ref pattern: ./common-types/foo.json → search in v1/schemas/{json,yaml}/
  const subdir = ext === ".json" ? "json" : "yaml";
  const nested = path.join(
    commonTypesLocalDir,
    "v1",
    "schemas",
    subdir,
    basename,
  );
  if (fs.existsSync(nested)) return nested;

  // Try opposite format
  const altSubdir = subdir === "json" ? "yaml" : "json";
  const altExt = altSubdir === "json" ? ".json" : ".yaml";
  const altBasename = basename.replace(ext, altExt);
  const alt = path.join(
    commonTypesLocalDir,
    "v1",
    "schemas",
    altSubdir,
    altBasename,
  );
  if (fs.existsSync(alt)) return alt;

  return null;
}

async function dereferenceSpec(specFilePath: string): Promise<OpenApiSpec> {
  const options: ParserOptions = {
    continueOnError: true,
    dereference: {
      circular: "ignore",
    },
    resolve: {
      // Disable built-in HTTP resolver to prevent outbound fetches.
      // All refs must resolve locally or via our custom resolvers below.
      http: false as unknown as ParserOptions["resolve"],
      // Custom resolver: map schemas.api.godaddy.com URLs to local files
      godaddySchemas: {
        order: 1,
        canRead: (file: { url: string }) => {
          try {
            const hostname = new URL(file.url).hostname;
            return hostname === "schemas.api.godaddy.com";
          } catch {
            return false;
          }
        },
        read: (file: { url: string }) => {
          if (!commonTypesLocalDir) {
            throw new Error(
              `Cannot resolve ${file.url}: common-types not cloned`,
            );
          }
          const urlPath = new URL(file.url).pathname;
          const localPath = path.join(
            commonTypesLocalDir,
            urlPath.replace(/^\/common-types\//, "/"),
          );
          if (!fs.existsSync(localPath)) {
            throw new Error(
              `Cannot resolve ${file.url}: not found at ${localPath}`,
            );
          }
          return fs.readFileSync(localPath, "utf-8");
        },
      },
      // Custom file resolver: intercept missing common-types paths
      commonTypesFile: {
        order: 200, // run after built-in file resolver (order 100)
        canRead: (file: { url: string }) => {
          return file.url.includes("common-types");
        },
        read: (file: { url: string }) => {
          // Convert file:// URL to path
          let filePath: string;
          try {
            filePath = fileURLToPath(file.url);
          } catch {
            filePath = file.url.replace(/^file:\/\//, "");
          }
          // If the file exists on disk, read it normally
          if (fs.existsSync(filePath)) {
            return fs.readFileSync(filePath, "utf-8");
          }
          // Otherwise try to resolve via common-types clone
          const resolved = resolveCommonTypesFile(filePath);
          if (resolved) {
            return fs.readFileSync(resolved, "utf-8");
          }
          throw new Error(
            `Cannot resolve common-types ref: ${path.basename(filePath)}`,
          );
        },
      },
    },
  };

  // Use an instance so we can access the partially-resolved schema
  // even when continueOnError throws after accumulating errors.
  const parser = new $RefParser();
  try {
    await parser.dereference(specFilePath, options);
  } catch (error) {
    // continueOnError accumulates errors then throws; the schema is still
    // partially resolved on the instance.
    const message = error instanceof Error ? error.message : String(error);
    console.error(
      `    WARNING: $ref resolution had errors: ${message.slice(0, 200)}`,
    );
  }

  if (parser.schema) {
    return parser.schema as unknown as OpenApiSpec;
  }

  // Total failure — fall back to raw parse
  const raw = fs.readFileSync(specFilePath, "utf-8");
  return parseOpenApiSpec(raw, specFilePath);
}

// ---------------------------------------------------------------------------
// GitHub discovery helpers
// ---------------------------------------------------------------------------

function parseRepoOverride(): string[] | null {
  const raw = process.env.API_CATALOG_REPOS?.trim();
  if (!raw) return null;

  const repos = raw
    .split(",")
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);

  return repos.length > 0 ? repos : null;
}

function parseRepoRefsOverride(): Map<string, string> {
  const raw = process.env.API_CATALOG_REPO_REFS?.trim();
  if (!raw) return new Map();

  const refs = new Map<string, string>();

  for (const entry of raw.split(",")) {
    const trimmed = entry.trim();
    if (!trimmed) continue;

    const separatorIndex = trimmed.indexOf("=");
    if (separatorIndex <= 0 || separatorIndex === trimmed.length - 1) {
      console.error(
        `WARNING: invalid API_CATALOG_REPO_REFS entry '${trimmed}' (expected repo=ref)`,
      );
      continue;
    }

    const repoName = trimmed.slice(0, separatorIndex).trim();
    const ref = trimmed.slice(separatorIndex + 1).trim();
    if (!repoName || !ref) continue;

    refs.set(repoName, ref);
  }

  return refs;
}

function includeLegacyLocationRepo(): boolean {
  const raw = process.env.API_CATALOG_INCLUDE_LEGACY_LOCATION;
  if (!raw) return true;
  return raw.toLowerCase() !== "false";
}

function githubHeaders(): Record<string, string> {
  const headers: Record<string, string> = {
    Accept: "application/vnd.github+json",
    "User-Agent": "godaddy-cli-api-catalog-generator",
  };

  const token = process.env.GITHUB_TOKEN?.trim();
  if (token) {
    headers.Authorization = `Bearer ${token}`;
  }

  return headers;
}

function isGitHubRepoObject(value: unknown): value is {
  name: string;
  clone_url: string;
  archived: boolean;
  disabled: boolean;
  private: boolean;
} {
  if (typeof value !== "object" || value === null) return false;
  const record = value as Record<string, unknown>;
  return (
    typeof record.name === "string" &&
    typeof record.clone_url === "string" &&
    typeof record.archived === "boolean" &&
    typeof record.disabled === "boolean" &&
    typeof record.private === "boolean"
  );
}

async function listReposForOwnerPath(ownerPath: string): Promise<GitHubRepo[]> {
  const repos: GitHubRepo[] = [];

  for (let page = 1; ; page += 1) {
    const url = `${GITHUB_API_BASE}/${ownerPath}/${GITHUB_ORG}/repos?per_page=${GITHUB_REPOS_PAGE_SIZE}&page=${page}&type=public&sort=full_name&direction=asc`;

    const response = await fetch(url, { headers: githubHeaders() });
    if (!response.ok) {
      if (response.status === 404) {
        return [];
      }
      throw new Error(
        `GitHub API request failed (${response.status}) while listing ${ownerPath} repos`,
      );
    }

    const payload = (await response.json()) as unknown;
    if (!Array.isArray(payload)) {
      throw new Error("Unexpected GitHub API response: expected an array");
    }

    if (payload.length === 0) break;

    for (const item of payload) {
      if (!isGitHubRepoObject(item)) {
        continue;
      }
      repos.push({
        name: item.name,
        cloneUrl: item.clone_url,
        archived: item.archived,
        disabled: item.disabled,
        private: item.private,
      });
    }

    if (payload.length < GITHUB_REPOS_PAGE_SIZE) {
      break;
    }
  }

  return repos;
}

async function listOrgRepos(): Promise<GitHubRepo[]> {
  const orgRepos = await listReposForOwnerPath("orgs");
  if (orgRepos.length > 0) {
    return orgRepos;
  }

  const userRepos = await listReposForOwnerPath("users");
  if (userRepos.length > 0) {
    return userRepos;
  }

  return [];
}

function deriveDomainFromRepoName(repoName: string): string {
  const withoutSuffix = repoName.endsWith("-specification")
    ? repoName.slice(0, -"-specification".length)
    : repoName;

  const withoutCommercePrefix = withoutSuffix.startsWith("commerce.")
    ? withoutSuffix.slice("commerce.".length)
    : withoutSuffix;

  return withoutCommercePrefix
    .toLowerCase()
    .replace(/\./g, "-")
    .replace(/[^a-z0-9-]/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-|-$/g, "");
}

function parseVersionDirectory(versionDirName: string): number[] | null {
  if (!/^v\d+(?:\.\d+)*$/.test(versionDirName)) {
    return null;
  }

  const numeric = versionDirName.slice(1);
  const parts = numeric.split(".").map((part) => Number.parseInt(part, 10));

  if (parts.some((part) => Number.isNaN(part))) {
    return null;
  }

  return parts;
}

function compareVersionArrays(a: number[], b: number[]): number {
  const length = Math.max(a.length, b.length);
  for (let index = 0; index < length; index += 1) {
    const aPart = a[index] ?? 0;
    const bPart = b[index] ?? 0;
    if (aPart !== bPart) return aPart - bPart;
  }
  return 0;
}

function findLatestSpecFile(
  repoDir: string,
): { version: string; specFile: string; graphqlOnly?: boolean } | null {
  const versionCandidates = fs
    .readdirSync(repoDir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .map((name) => ({ name, parsed: parseVersionDirectory(name) }))
    .filter(
      (entry): entry is { name: string; parsed: number[] } =>
        entry.parsed !== null,
    )
    .sort((left, right) => compareVersionArrays(left.parsed, right.parsed));

  for (let index = versionCandidates.length - 1; index >= 0; index -= 1) {
    const version = versionCandidates[index].name;

    // Prefer OpenAPI specs
    const openApiCandidates = [
      path.join(repoDir, version, "schemas", "openapi.yaml"),
      path.join(repoDir, version, "schemas", "openapi.yml"),
      path.join(repoDir, version, "schemas", "openapi.json"),
    ];

    for (const candidate of openApiCandidates) {
      if (fs.existsSync(candidate)) {
        return { version, specFile: candidate };
      }
    }

    // Fall back to standalone GraphQL schema
    const graphqlCandidates = [
      path.join(repoDir, version, "schemas", "graphql", "schema.graphql"),
      path.join(repoDir, version, "schemas", "schema.graphql"),
    ];

    for (const candidate of graphqlCandidates) {
      if (fs.existsSync(candidate)) {
        return { version, specFile: candidate, graphqlOnly: true };
      }
    }
  }

  return null;
}

function checkoutRepositoryRef(targetDir: string, ref: string): void {
  execFileSync(
    "git",
    ["-C", targetDir, "fetch", "--depth", "1", "origin", ref],
    {
      stdio: "pipe",
      env: process.env,
    },
  );

  execFileSync("git", ["-C", targetDir, "checkout", "--quiet", "FETCH_HEAD"], {
    stdio: "pipe",
    env: process.env,
  });
}

function cloneRepository(
  cloneUrl: string,
  targetDir: string,
  ref?: string,
): void {
  execFileSync(
    "git",
    ["clone", "--depth", "1", "--quiet", cloneUrl, targetDir],
    {
      stdio: "pipe",
      env: process.env,
    },
  );

  if (ref) {
    checkoutRepositoryRef(targetDir, ref);
  }
}

async function discoverSpecSources(): Promise<DiscoveredSpecSources> {
  const allRepos = await listOrgRepos();
  const repoMap = new Map(allRepos.map((repo) => [repo.name, repo]));

  const overrides = parseRepoOverride();
  const repoRefOverrides = parseRepoRefsOverride();

  const selectedRepoNames = new Set<string>();
  if (overrides) {
    for (const repoName of overrides) {
      selectedRepoNames.add(repoName);
    }
  } else {
    for (const repo of allRepos) {
      if (COMMERCE_SPEC_REPO_PATTERN.test(repo.name)) {
        selectedRepoNames.add(repo.name);
      }
    }
  }

  if (!overrides && selectedRepoNames.size === 0) {
    console.error(
      "WARNING: Dynamic GitHub discovery found no commerce specifications. Falling back to bootstrap repository list.",
    );
    for (const repoName of BOOTSTRAP_COMMERCE_REPOS) {
      selectedRepoNames.add(repoName);
    }
  }

  if (includeLegacyLocationRepo()) {
    for (const legacyRepo of LEGACY_ALWAYS_INCLUDE_REPOS) {
      selectedRepoNames.add(legacyRepo);
    }
  }

  const selectedRepos = [...selectedRepoNames]
    .map((name) => {
      const discovered = repoMap.get(name);
      if (discovered) {
        return discovered;
      }

      return {
        name,
        cloneUrl: `https://github.com/${GITHUB_ORG}/${name}.git`,
        archived: false,
        disabled: false,
        private: false,
      } satisfies GitHubRepo;
    })
    .filter((repo) => !repo.private)
    .filter((repo) => !repo.archived)
    .filter((repo) => !repo.disabled)
    .sort((a, b) => a.name.localeCompare(b.name));

  if (!overrides && allRepos.length === 0) {
    console.error(
      "WARNING: GitHub repo discovery returned 0 repositories. Set API_CATALOG_REPOS or provide GITHUB_TOKEN for broader discovery.",
    );
  }

  if (selectedRepos.length === 0) {
    return { sources: [], cloneRoot: null };
  }

  const cloneRoot = fs.mkdtempSync(
    path.join(os.tmpdir(), "godaddy-api-catalog-"),
  );
  const sources: SpecSource[] = [];
  const usedDomains = new Set<string>();

  // Clone common-types-specification once; it is referenced as a submodule
  // by most commerce spec repos at v*/schemas/common-types.
  const commonTypesDir = path.join(cloneRoot, "__common-types");
  try {
    cloneRepository(
      `https://github.com/${GITHUB_ORG}/common-types-specification.git`,
      commonTypesDir,
    );
    commonTypesLocalDir = commonTypesDir;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error(
      `WARNING: failed to clone common-types-specification: ${message}`,
    );
  }

  for (const repo of selectedRepos) {
    const repoDir = path.join(cloneRoot, repo.name);
    const repoRef = repoRefOverrides.get(repo.name);

    try {
      cloneRepository(repo.cloneUrl, repoDir, repoRef);
      if (repoRef) {
        console.log(`  ${repo.name}: checked out override ref '${repoRef}'`);
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      const context = repoRef ? ` @ ref '${repoRef}'` : "";
      console.error(
        `WARNING: failed to clone ${repo.name}${context}: ${message}`,
      );
      continue;
    }

    const latestSpec = findLatestSpecFile(repoDir);
    if (!latestSpec) {
      console.error(
        `WARNING: ${repo.name} has no v*/schemas/openapi.{yaml,yml,json} — skipping`,
      );
      continue;
    }

    const domain = deriveDomainFromRepoName(repo.name);
    if (!domain) {
      console.error(
        `WARNING: could not derive domain key from repo '${repo.name}' — skipping`,
      );
      continue;
    }

    if (usedDomains.has(domain)) {
      console.error(
        `WARNING: duplicate derived domain '${domain}' from repo '${repo.name}' — skipping`,
      );
      continue;
    }

    usedDomains.add(domain);
    sources.push({
      domain,
      repoName: repo.name,
      specFile: latestSpec.specFile,
      specVersion: latestSpec.version,
      graphqlOnly: latestSpec.graphqlOnly,
    });
  }

  return { sources, cloneRoot };
}

// ---------------------------------------------------------------------------
// Spec processing helpers
// ---------------------------------------------------------------------------

const HTTP_METHODS = new Set([
  "get",
  "post",
  "put",
  "patch",
  "delete",
  "options",
  "head",
  "trace",
]);

function resolveBaseUrl(servers?: OpenApiServer[]): string {
  if (!servers || servers.length === 0) return "";
  const server = servers[0];
  let url = server.url;
  if (server.variables) {
    for (const [key, variable] of Object.entries(server.variables)) {
      url = url.replace(`{${key}}`, variable.default);
    }
  }
  return url;
}

const COMMERCE_SCOPE_URN_REGEX =
  /^urn:godaddy:services:commerce\.([a-z0-9-]+):([a-z-]+)$/i;
const COMMERCE_SCOPE_URI_REGEX =
  /^https:\/\/uri\.godaddy\.com\/services\/commerce\/([a-z0-9-]+)\/([a-z-]+)$/i;

function normalizeScopeAction(action: string): string {
  const normalized = action.toLowerCase();
  if (normalized === "read-write") {
    return "write";
  }
  return normalized;
}

function normalizeScopeToken(scope: string): string {
  const trimmed = scope.trim();
  if (!trimmed) return trimmed;

  const urnMatch = trimmed.match(COMMERCE_SCOPE_URN_REGEX);
  if (urnMatch) {
    const domain = urnMatch[1].toLowerCase();
    const action = normalizeScopeAction(urnMatch[2]);
    return `commerce.${domain}:${action}`;
  }

  const uriMatch = trimmed.match(COMMERCE_SCOPE_URI_REGEX);
  if (uriMatch) {
    const domain = uriMatch[1].toLowerCase();
    const action = normalizeScopeAction(uriMatch[2]);
    return `commerce.${domain}:${action}`;
  }

  const commerceMatch = trimmed.match(/^commerce\.([a-z0-9-]+):([a-z-]+)$/i);
  if (commerceMatch) {
    const domain = commerceMatch[1].toLowerCase();
    const action = normalizeScopeAction(commerceMatch[2]);
    return `commerce.${domain}:${action}`;
  }

  return trimmed;
}

function extractScopes(security?: Array<Record<string, string[]>>): string[] {
  if (!security) return [];

  const normalizedScopes = new Set<string>();

  for (const entry of security) {
    for (const scopeList of Object.values(entry)) {
      for (const rawScope of scopeList) {
        const normalizedScope = normalizeScopeToken(rawScope);
        if (normalizedScope) {
          normalizedScopes.add(normalizedScope);
        }
      }
    }
  }

  return [...normalizedScopes];
}

const graphqlSchemaCache = new Map<string, CatalogGraphqlSchema>();

function graphqlTypeToString(typeNode: TypeNode): string {
  switch (typeNode.kind) {
    case Kind.NAMED_TYPE:
      return typeNode.name.value;
    case Kind.NON_NULL_TYPE:
      return `${graphqlTypeToString(typeNode.type)}!`;
    case Kind.LIST_TYPE:
      return `[${graphqlTypeToString(typeNode.type)}]`;
  }
}

function parseGraphqlOperations(
  schemaSource: string,
): CatalogGraphqlOperation[] {
  const document = parseGraphql(schemaSource, { noLocation: true });
  const operations: CatalogGraphqlOperation[] = [];

  for (const definition of document.definitions) {
    if (definition.kind !== Kind.OBJECT_TYPE_DEFINITION) continue;

    const typeName = definition.name.value;
    if (typeName !== "Query" && typeName !== "Mutation") continue;

    const kind: CatalogGraphqlOperation["kind"] =
      typeName === "Query" ? "query" : "mutation";

    for (const field of definition.fields ?? []) {
      const deprecatedDirective = field.directives?.find(
        (directive) => directive.name.value === "deprecated",
      );
      const deprecationReasonArg = deprecatedDirective?.arguments?.find(
        (arg) => arg.name.value === "reason",
      );

      const deprecationReason = deprecationReasonArg
        ? deprecationReasonArg.value.kind === Kind.STRING
          ? deprecationReasonArg.value.value
          : printGraphql(deprecationReasonArg.value)
        : undefined;

      const args: CatalogGraphqlArgument[] = (field.arguments ?? []).map(
        (arg) => ({
          name: arg.name.value,
          type: graphqlTypeToString(arg.type),
          required:
            arg.type.kind === Kind.NON_NULL_TYPE &&
            arg.defaultValue === undefined,
          description: arg.description?.value,
          defaultValue:
            arg.defaultValue === undefined
              ? undefined
              : printGraphql(arg.defaultValue),
        }),
      );

      operations.push({
        name: field.name.value,
        kind,
        returnType: graphqlTypeToString(field.type),
        description: field.description?.value,
        deprecated: deprecatedDirective !== undefined,
        deprecationReason,
        args,
      });
    }
  }

  return operations.sort((left, right) => {
    if (left.kind === right.kind) {
      return left.name.localeCompare(right.name);
    }
    return left.kind === "query" ? -1 : 1;
  });
}

function loadGraphqlSchemaMetadata(
  specFile: string,
  schemaRef: string,
): CatalogGraphqlSchema {
  const specDir = path.dirname(specFile);
  const resolvedSchemaPath = path.resolve(specDir, schemaRef);
  const cacheKey = `${specFile}::${resolvedSchemaPath}`;

  const cached = graphqlSchemaCache.get(cacheKey);
  if (cached) return cached;

  if (!fs.existsSync(resolvedSchemaPath)) {
    throw new Error(
      `GraphQL schema file not found for '${schemaRef}' (resolved: ${resolvedSchemaPath})`,
    );
  }

  const schemaSource = fs.readFileSync(resolvedSchemaPath, "utf-8");

  let operations: CatalogGraphqlOperation[];
  try {
    operations = parseGraphqlOperations(schemaSource);
  } catch (parseError) {
    // GraphQL schemas in the wild sometimes contain syntax issues
    // (e.g. consecutive block-string descriptions without a field).
    // Try a best-effort repair: strip orphaned doc-comment blocks.
    const repaired = schemaSource.replace(
      /"""[^"]*"""\s*\n\s*"""/g,
      (match) => {
        // Keep only the last doc-comment block
        const lastIdx = match.lastIndexOf('"""', match.length - 4);
        const secondLastIdx = match.lastIndexOf('"""', lastIdx - 1);
        return match.slice(secondLastIdx);
      },
    );
    try {
      operations = parseGraphqlOperations(repaired);
      console.error(
        `WARNING: repaired malformed GraphQL schema at ${resolvedSchemaPath}`,
      );
    } catch {
      console.error(
        `WARNING: could not parse GraphQL schema at ${resolvedSchemaPath}: ${
          parseError instanceof Error ? parseError.message : String(parseError)
        }`,
      );
      operations = [];
    }
  }

  const metadata: CatalogGraphqlSchema = {
    schemaRef,
    operationCount: operations.length,
    operations,
  };

  graphqlSchemaCache.set(cacheKey, metadata);
  return metadata;
}

function resolveLocalRef(
  spec: OpenApiSpec,
  ref: string,
): Record<string, unknown> | null {
  if (!ref.startsWith("#/")) return null;

  const segments = ref
    .slice(2)
    .split("/")
    .map((segment) => segment.replace(/~1/g, "/").replace(/~0/g, "~"));

  let current: unknown = spec as unknown;
  for (const segment of segments) {
    if (typeof current !== "object" || current === null) {
      return null;
    }

    const record = current as Record<string, unknown>;
    current = record[segment];
  }

  if (typeof current !== "object" || current === null) {
    return null;
  }

  return current as Record<string, unknown>;
}

function resolveParameter(
  spec: OpenApiSpec,
  parameter: OpenApiParameter | OpenApiReference,
): OpenApiParameter | null {
  if (!parameter || typeof parameter !== "object") return null;
  if ("$ref" in parameter) {
    const resolved = resolveLocalRef(spec, parameter.$ref);
    if (!resolved) return null;

    const name = resolved.name;
    const location = resolved.in;
    if (typeof name !== "string" || typeof location !== "string") {
      return null;
    }

    return {
      name,
      in: location,
      required:
        typeof resolved.required === "boolean" ? resolved.required : undefined,
      description:
        typeof resolved.description === "string"
          ? resolved.description
          : undefined,
      schema:
        typeof resolved.schema === "object" && resolved.schema !== null
          ? (resolved.schema as Record<string, unknown>)
          : undefined,
    };
  }

  return parameter;
}

function processOperation(
  spec: OpenApiSpec,
  specFile: string,
  httpMethod: string,
  pathStr: string,
  operation: OpenApiOperation,
  pathLevelParams?: Array<OpenApiParameter | OpenApiReference>,
): CatalogEndpoint {
  const allParams = [
    ...(pathLevelParams || []),
    ...(operation.parameters || []),
  ];

  const parameters = allParams
    .map((parameter) => resolveParameter(spec, parameter))
    .filter((parameter): parameter is OpenApiParameter => parameter !== null)
    .map((parameter) => ({
      name: parameter.name,
      in: parameter.in,
      required: parameter.required ?? false,
      description: parameter.description,
      schema: parameter.schema,
    }));

  let requestBody: CatalogEndpoint["requestBody"];
  if (operation.requestBody && !("$ref" in operation.requestBody)) {
    const rb = operation.requestBody;
    const contentTypes = rb.content ? Object.keys(rb.content) : [];
    const primaryCt = contentTypes[0] || "application/json";
    const schema = rb.content?.[primaryCt]?.schema;

    requestBody = {
      required: rb.required ?? false,
      description: rb.description,
      contentType: primaryCt,
      schema,
    };
  }

  const responses: CatalogEndpoint["responses"] = {};
  if (operation.responses) {
    for (const [status, resp] of Object.entries(operation.responses)) {
      if (!resp || typeof resp !== "object") continue;
      if ("$ref" in resp) {
        responses[status] = {
          description: `See ${(resp as { $ref: string }).$ref}`,
        };
        continue;
      }
      const contentTypes = resp.content ? Object.keys(resp.content) : [];
      const primaryCt = contentTypes[0] || "application/json";
      responses[status] = {
        description: resp.description || "",
        schema: resp.content?.[primaryCt]?.schema,
      };
    }
  }

  const operationId =
    operation.operationId ||
    `${httpMethod}_${pathStr
      .replace(/[^a-zA-Z0-9]/g, "_")
      .replace(/_+/g, "_")
      .replace(/^_|_$/g, "")}`;

  let graphql: CatalogGraphqlSchema | undefined;
  const graphqlSchemaRef = operation["x-godaddy-graphql-schema"];
  if (typeof graphqlSchemaRef === "string" && graphqlSchemaRef.length > 0) {
    graphql = loadGraphqlSchemaMetadata(specFile, graphqlSchemaRef);
  }

  return {
    operationId,
    method: httpMethod.toUpperCase(),
    path: pathStr,
    summary: operation.summary || "",
    description: operation.description,
    parameters: parameters.length > 0 ? parameters : undefined,
    requestBody,
    responses,
    scopes: extractScopes(operation.security),
    graphql,
  };
}

function processSpec(
  spec: OpenApiSpec,
  domain: string,
  specFile: string,
): CatalogDomain {
  const baseUrl = resolveBaseUrl(spec.servers);
  const endpoints: CatalogEndpoint[] = [];

  for (const [pathStr, pathItem] of Object.entries(spec.paths)) {
    const pathLevelParams = pathItem.parameters;

    for (const [key, value] of Object.entries(pathItem)) {
      if (key === "parameters" || !HTTP_METHODS.has(key) || !value) continue;
      const operation = value as OpenApiOperation;
      endpoints.push(
        processOperation(
          spec,
          specFile,
          key,
          pathStr,
          operation,
          pathLevelParams,
        ),
      );
    }
  }

  return {
    name: domain,
    title: spec.info.title,
    description: spec.info.description || "",
    version: spec.info.version,
    baseUrl,
    endpoints,
  };
}

function repairMissingJsonCommas(raw: string): string {
  const lines = raw.split("\n");

  for (let index = 0; index < lines.length - 1; index += 1) {
    const currentLine = lines[index];
    const nextLine = lines[index + 1];

    const currentTrimmedRight = currentLine.trimEnd();
    const nextTrimmedLeft = nextLine.trimStart();

    if (currentTrimmedRight.length === 0) continue;
    if (/[,\[{]\s*$/.test(currentTrimmedRight)) continue;
    if (!/^"[^"\\]+"\s*:/.test(nextTrimmedLeft)) continue;
    if (!/^\s*"[^"\\]+"\s*:/.test(currentTrimmedRight)) continue;
    if (!/["}\]0-9a-zA-Z]$/.test(currentTrimmedRight)) continue;

    lines[index] = `${currentLine},`;
  }

  return lines.join("\n");
}

/**
 * Repair YAML lines where a colon is missing the required trailing space,
 * e.g. `operationId:disableWebScannerAlert` → `operationId: disableWebScannerAlert`.
 */
function repairMissingYamlColonSpace(raw: string): string {
  // Only match horizontal whitespace (spaces/tabs) so we don't span lines
  return raw.replace(
    /^([ \t]+\w+):([^\s#])/gm,
    (_, key: string, val: string) => `${key}: ${val}`,
  );
}

function parseOpenApiSpec(raw: string, specFile: string): OpenApiSpec {
  try {
    return parseYaml(raw) as OpenApiSpec;
  } catch (error) {
    // Try YAML colon-space repair first (works for both .yaml and .json)
    const yamlRepaired = repairMissingYamlColonSpace(raw);
    if (yamlRepaired !== raw) {
      try {
        console.error(`WARNING: repaired missing colon-space in ${specFile}`);
        return parseYaml(yamlRepaired) as OpenApiSpec;
      } catch {
        // fall through to JSON repair
      }
    }

    const lowerPath = specFile.toLowerCase();
    if (!lowerPath.endsWith(".json")) {
      throw error;
    }

    const repaired = repairMissingJsonCommas(raw);
    if (repaired === raw) {
      throw error;
    }

    try {
      console.error(
        `WARNING: detected malformed JSON in ${specFile}; attempting comma-repair fallback`,
      );
      return parseYaml(repaired) as OpenApiSpec;
    } catch {
      throw error;
    }
  }
}

function identifierForDomain(domain: string): string {
  const clean = domain.replace(/[^a-zA-Z0-9]+/g, "_").replace(/^_+|_+$/g, "");

  if (!clean) return "domain_json";
  if (/^[0-9]/.test(clean)) return `domain_${clean}`;
  return `${clean}_json`;
}

function writeRegistryFile(domains: string[]): void {
  const sorted = [...domains].sort((a, b) => a.localeCompare(b));

  const importLines = sorted.map((domain) => {
    const identifier = identifierForDomain(domain);
    return `import ${identifier} from "./${domain}.json";`;
  });

  const objectLines = sorted.map((domain) => {
    const identifier = identifierForDomain(domain);
    return `  "${domain}": ${identifier},`;
  });

  const content = [
    "/**",
    " * AUTO-GENERATED by scripts/generate-api-catalog.ts",
    " * Do not edit manually.",
    " */",
    "",
    ...importLines,
    "",
    "export const DOMAIN_REGISTRY: Record<string, unknown> = {",
    ...objectLines,
    "};",
    "",
  ].join("\n");

  fs.writeFileSync(REGISTRY_FILE, content, "utf-8");
}

function removeStaleDomainJsonFiles(activeDomainFiles: Set<string>): void {
  const entries = fs.readdirSync(OUTPUT_DIR, { withFileTypes: true });
  for (const entry of entries) {
    if (!entry.isFile()) continue;
    if (!entry.name.endsWith(".json")) continue;
    if (entry.name === "manifest.json") continue;

    if (!activeDomainFiles.has(entry.name)) {
      fs.rmSync(path.join(OUTPUT_DIR, entry.name), {
        recursive: false,
        force: true,
      });
    }
  }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  fs.mkdirSync(OUTPUT_DIR, { recursive: true });

  const { sources: specSources, cloneRoot } = await discoverSpecSources();

  const manifest: CatalogManifest = {
    generated: new Date().toISOString(),
    domains: {},
  };

  const activeDomainFiles = new Set<string>();
  let totalEndpoints = 0;

  try {
    if (specSources.length === 0) {
      throw new Error(
        "No specification repositories discovered. Refusing to overwrite catalog output.",
      );
    }

    for (const source of specSources) {
      if (!fs.existsSync(source.specFile)) {
        throw new Error(
          `spec file not found for ${source.repoName}: ${source.specFile}`,
        );
      }

      try {
        let catalog: CatalogDomain;

        if (source.graphqlOnly) {
          // GraphQL-only source — synthesize a catalog from the schema
          const gqlMeta = loadGraphqlSchemaMetadata(
            source.specFile,
            source.specFile,
          );
          // Replace the absolute temp path with a clean relative ref
          gqlMeta.schemaRef = "./schema.graphql";
          catalog = {
            name: source.domain,
            title: `${source.domain} GraphQL API`,
            description: `GraphQL API with ${gqlMeta.operationCount} operations`,
            version: source.specVersion.replace(/^v/, ""),
            baseUrl: "",
            endpoints: [
              {
                operationId: "graphql",
                method: "POST",
                path: "/graphql",
                summary: "GraphQL API",
                description: `GraphQL endpoint with ${gqlMeta.operationCount} operations`,
                responses: {
                  "200": { description: "GraphQL response" },
                },
                scopes: [],
                graphql: gqlMeta,
              },
            ],
          };
          console.log(
            `  ${source.domain}: GraphQL schema with ${gqlMeta.operationCount} operations (${source.repoName}/${source.specVersion})`,
          );
        } else {
          console.log(
            `  Dereferencing ${source.domain} (${source.repoName}/${source.specVersion})...`,
          );
          const spec = await dereferenceSpec(source.specFile);
          catalog = processSpec(spec, source.domain, source.specFile);
        }

        const filename = `${source.domain}.json`;
        activeDomainFiles.add(filename);

        fs.writeFileSync(
          path.join(OUTPUT_DIR, filename),
          JSON.stringify(catalog, null, 2),
          "utf-8",
        );

        manifest.domains[source.domain] = {
          file: filename,
          title: catalog.title,
          endpointCount: catalog.endpoints.length,
        };

        totalEndpoints += catalog.endpoints.length;
        console.log(
          `  ${source.domain}: ${catalog.endpoints.length} endpoints from ${catalog.title} v${catalog.version}`,
        );
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        throw new Error(
          `failed processing ${source.repoName} (${source.specVersion}): ${message}`,
        );
      }
    }
  } finally {
    if (cloneRoot) {
      fs.rmSync(cloneRoot, { recursive: true, force: true });
    }
  }

  removeStaleDomainJsonFiles(activeDomainFiles);

  fs.writeFileSync(
    path.join(OUTPUT_DIR, "manifest.json"),
    JSON.stringify(manifest, null, 2),
    "utf-8",
  );

  writeRegistryFile(Object.keys(manifest.domains));

  console.log(
    `\nGenerated API catalog: ${Object.keys(manifest.domains).length} domains, ${totalEndpoints} endpoints`,
  );
}

main().catch((error) => {
  console.error("Fatal error:", error);
  process.exit(1);
});
