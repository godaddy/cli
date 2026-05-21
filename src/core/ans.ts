/**
 * ANS (Agent Name Service) REST API client.
 *
 * Authentication: API key via GODADDY_KEY + GODADDY_SECRET env vars.
 *
 * NOTE (auth gap): The existing CLI uses OAuth Bearer tokens via `godaddy auth
 * login`. ANS endpoints also accept Bearer tokens, but the current OAuth flow
 * does not request ANS-specific scopes. Until the required scope is known and
 * added to the CLI's auth flow, ANS commands authenticate via API key only.
 * See the PR description for discussion.
 */

import type { FileSystem } from "@effect/platform/FileSystem";
import * as Effect from "effect/Effect";
import {
  ConfigurationError,
  NetworkError,
  ServerError,
} from "../effect/errors";
import { type Environment, envGetEffect, getApiUrl } from "./environment";

// ---------------------------------------------------------------------------
// ANS response types
// ---------------------------------------------------------------------------

export interface DnsRecord {
  type: string;
  name: string;
  value: string;
  ttl?: number;
}

export interface NextStep {
  action: string;
  description: string;
  endpoint?: string;
  estimatedTimeMinutes?: number;
}

export interface RegistrationPending {
  agentId?: string;
  status: string;
  expiresAt?: string;
  dnsRecords?: DnsRecord[];
  nextSteps?: NextStep;
}

export interface AgentStatus {
  agentId: string;
  ansName: string;
  status: string;
  failureReason?: string;
  nextSteps?: NextStep;
  dnsRecords?: DnsRecord[];
  createdAt?: string;
  updatedAt?: string;
  expiresAt?: string;
}

export interface CsrResponse {
  csrId: string;
  status: string;
}

export interface CsrStatus {
  csrId: string;
  status: string;
  failureReason?: string;
  submittedAt?: string;
  updatedAt?: string;
}

export interface AgentSummary {
  agentId: string;
  ansName: string;
  agentDisplayName: string;
  agentDescription?: string;
  status: string;
}

export interface AgentSearchResult {
  agents: AgentSummary[];
  total?: number;
}

export interface EventPage {
  events: AnsEvent[];
  nextLogId?: string;
}

export interface AnsEvent {
  logId: string;
  type: string;
  agentId?: string;
  timestamp: string;
  details?: unknown;
}

export interface BadgeEntry {
  agentId: string;
  certificate?: string;
  merkleProof?: unknown;
  timestamp?: string;
}

export interface CertList {
  certificates: string[];
}

export interface AgentEndpoint {
  url: string;
  protocol: string;
  transports?: string[];
}

export interface AgentRegistrationRequest {
  agentDisplayName: string;
  agentHost: string;
  version: string;
  identityCsrPem: string;
  serverCsrPem?: string;
  endpoints: AgentEndpoint[];
}

export type RevocationReason =
  | "CESSATION_OF_OPERATION"
  | "KEY_COMPROMISE"
  | "AFFILIATION_CHANGED"
  | "SUPERSEDED"
  | "EXPIRED_CERT"
  | "UNSPECIFIED";

export const REVOCATION_REASONS: RevocationReason[] = [
  "CESSATION_OF_OPERATION",
  "KEY_COMPROMISE",
  "AFFILIATION_CHANGED",
  "SUPERSEDED",
  "EXPIRED_CERT",
  "UNSPECIFIED",
];

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

type AnsService = "registry" | "transparency";

function getAnsBaseUrl(env: Environment, service: AnsService): string {
  if (service === "transparency") {
    return env === "prod"
      ? "https://transparency.ans.godaddy.com"
      : "https://transparency.ans.ote-godaddy.com";
  }
  return getApiUrl(env);
}

function getAnsAuthHeader(): Effect.Effect<string, ConfigurationError, never> {
  const key = process.env.GODADDY_KEY;
  const secret = process.env.GODADDY_SECRET;
  if (!key || !secret) {
    return Effect.fail(
      new ConfigurationError({
        message: "GODADDY_KEY and GODADDY_SECRET are required for ANS commands",
        userMessage:
          "ANS commands require API key credentials. Set GODADDY_KEY and GODADDY_SECRET environment variables. Obtain credentials at https://developer.godaddy.com/keys",
      }),
    );
  }
  return Effect.succeed(`sso-key ${key}:${secret}`);
}

type AnsError = ConfigurationError | NetworkError | ServerError;

function makeAnsRequest<T>(
  method: "GET" | "POST" | "DELETE",
  path: string,
  body?: unknown,
  service: AnsService = "registry",
): Effect.Effect<T, AnsError, FileSystem> {
  return Effect.gen(function* () {
    const authHeader = yield* getAnsAuthHeader();
    const env = yield* envGetEffect().pipe(
      Effect.mapError(
        (e) =>
          new ConfigurationError({
            message: `Failed to determine environment: ${e.message}`,
            userMessage: "Could not determine target environment",
          }),
      ),
    );

    const baseUrl = getAnsBaseUrl(env, service);
    const url = `${baseUrl}${path}`;

    const headers: Record<string, string> = {
      Authorization: authHeader,
      Accept: "application/json",
    };

    if (body !== undefined) {
      headers["Content-Type"] = "application/json";
    } else if (method === "POST") {
      // Akamai edge layer requires Content-Length: 0 on bodyless POST requests.
      // Without it the request is rejected with HTTP 411.
      headers["Content-Length"] = "0";
    }

    const response = yield* Effect.tryPromise({
      try: () =>
        globalThis.fetch(url, {
          method,
          headers,
          body: body !== undefined ? JSON.stringify(body) : undefined,
        }),
      catch: (e) =>
        new NetworkError({
          message: `ANS request failed: ${String(e)}`,
          userMessage: "Could not reach ANS API",
          endpoint: path,
          method,
        }),
    });

    if (!response.ok) {
      const errorBody = yield* Effect.tryPromise({
        try: () => response.json() as Promise<{ message?: string }>,
        catch: (e) =>
          new NetworkError({
            message: `Failed to parse error response: ${String(e)}`,
            userMessage: "Unexpected error response from ANS API",
            endpoint: path,
            method,
          }),
      }).pipe(Effect.orElse(() => Effect.succeed({} as { message?: string })));
      const message =
        errorBody.message ??
        `ANS API error: ${response.status} ${response.statusText}`;
      return yield* Effect.fail(
        new ServerError({
          kind:
            response.status === 404
              ? "NOT_FOUND"
              : response.status === 409
                ? "CONFLICT"
                : response.status === 403
                  ? "FORBIDDEN"
                  : response.status === 429
                    ? "RATE_LIMITED"
                    : "VALIDATION",
          message,
          userMessage: message,
          status: response.status,
          statusText: response.statusText,
          endpoint: path,
          method,
        }),
      );
    }

    // 204 No Content or empty body
    const contentLength = response.headers.get("content-length");
    if (response.status === 204 || contentLength === "0") {
      return undefined as T;
    }

    return yield* Effect.tryPromise({
      try: () => response.json() as Promise<T>,
      catch: (e) =>
        new NetworkError({
          message: `Failed to parse ANS response: ${String(e)}`,
          userMessage: "Unexpected response format from ANS API",
          endpoint: path,
          method,
        }),
    });
  });
}

// ---------------------------------------------------------------------------
// Registry API effects
// ---------------------------------------------------------------------------

export function registerAgentEffect(
  req: AgentRegistrationRequest,
): Effect.Effect<RegistrationPending, AnsError, FileSystem> {
  return makeAnsRequest("POST", "/v1/agents/register", req);
}

export function getAgentStatusEffect(
  agentId: string,
): Effect.Effect<AgentStatus, AnsError, FileSystem> {
  return makeAnsRequest("GET", `/v1/agents/${agentId}`);
}

export function verifyAcmeEffect(
  agentId: string,
): Effect.Effect<AgentStatus, AnsError, FileSystem> {
  return makeAnsRequest("POST", `/v1/agents/${agentId}/verify-acme`);
}

export function verifyDnsEffect(
  agentId: string,
): Effect.Effect<AgentStatus, AnsError, FileSystem> {
  return makeAnsRequest("POST", `/v1/agents/${agentId}/verify-dns`);
}

export function submitServerCsrEffect(
  agentId: string,
  csrPem: string,
): Effect.Effect<CsrResponse, AnsError, FileSystem> {
  return makeAnsRequest("POST", `/v1/agents/${agentId}/certificates/server`, {
    csrPem,
  });
}

export function submitIdentityCsrEffect(
  agentId: string,
  csrPem: string,
): Effect.Effect<CsrResponse, AnsError, FileSystem> {
  return makeAnsRequest("POST", `/v1/agents/${agentId}/certificates/identity`, {
    csrPem,
  });
}

export function getCsrStatusEffect(
  agentId: string,
  csrId: string,
): Effect.Effect<CsrStatus, AnsError, FileSystem> {
  return makeAnsRequest("GET", `/v1/agents/${agentId}/csrs/${csrId}/status`);
}

export function revokeAgentEffect(
  agentId: string,
  reason: RevocationReason,
): Effect.Effect<void, AnsError, FileSystem> {
  return makeAnsRequest("POST", `/v1/agents/${agentId}/revoke`, { reason });
}

export function searchAgentsEffect(criteria: {
  host?: string;
  name?: string;
  version?: string;
}): Effect.Effect<AgentSearchResult, AnsError, FileSystem> {
  const params = new URLSearchParams();
  if (criteria.host) params.set("agentHost", criteria.host);
  if (criteria.name) params.set("agentDisplayName", criteria.name);
  if (criteria.version) params.set("version", criteria.version);
  const qs = params.toString();
  return makeAnsRequest("GET", `/v1/agents${qs ? `?${qs}` : ""}`);
}

export function resolveAgentEffect(
  host: string,
  version?: string,
): Effect.Effect<AgentSearchResult, AnsError, FileSystem> {
  const params = new URLSearchParams({ agentHost: host });
  if (version) params.set("version", version);
  return makeAnsRequest("GET", `/v1/agents/resolve?${params.toString()}`);
}

export function getEventsEffect(opts: {
  limit?: number;
  lastLogId?: string;
}): Effect.Effect<EventPage, AnsError, FileSystem> {
  const params = new URLSearchParams();
  if (opts.limit !== undefined) params.set("limit", String(opts.limit));
  if (opts.lastLogId) params.set("lastLogId", opts.lastLogId);
  const qs = params.toString();
  return makeAnsRequest("GET", `/v1/agents/events${qs ? `?${qs}` : ""}`);
}

export function getServerCertsEffect(
  agentId: string,
): Effect.Effect<CertList, AnsError, FileSystem> {
  return makeAnsRequest("GET", `/v1/agents/${agentId}/certificates/server`);
}

export function getIdentityCertsEffect(
  agentId: string,
): Effect.Effect<CertList, AnsError, FileSystem> {
  return makeAnsRequest("GET", `/v1/agents/${agentId}/certificates/identity`);
}

// ---------------------------------------------------------------------------
// Transparency log API effects
// ---------------------------------------------------------------------------

export function getBadgeEffect(
  agentId: string,
): Effect.Effect<BadgeEntry, AnsError, FileSystem> {
  return makeAnsRequest(
    "GET",
    `/v1/agents/${agentId}/badge`,
    undefined,
    "transparency",
  );
}
