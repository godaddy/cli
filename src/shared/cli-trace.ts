import { v7 as uuid } from "uuid";
import packageJson from "../../package.json";

/** Shared User-Agent for outbound CLI HTTP requests (server-side tracing). */
export const CLI_USER_AGENT = `godaddy-cli/${packageJson.version}`;

/**
 * Headers attached to outbound requests so upstream logs can correlate calls.
 * Each invocation generates a fresh request ID.
 *
 * Keys are lowercase (`user-agent`, `x-request-id`) so the same spelling is
 * used as in `ensureRequiredRequestHeaders` (`src/core/api.ts`) and across
 * REST, GraphQL, OAuth, and webhook calls. HTTP treats header names as
 * case-insensitive; lowercase keeps log pipelines and tests consistent.
 */
export function cliTraceHeaders(): Record<string, string> {
  return {
    "user-agent": CLI_USER_AGENT,
    "x-request-id": uuid(),
  };
}
