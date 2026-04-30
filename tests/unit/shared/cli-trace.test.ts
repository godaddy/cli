import { describe, expect, test } from "vitest";
import packageJson from "../../../package.json";
import { getRequestHeaders } from "../../../src/services/http-helpers";
import { CLI_USER_AGENT, cliTraceHeaders } from "../../../src/shared/cli-trace";

describe("cli-trace", () => {
  test("CLI_USER_AGENT includes package version", () => {
    expect(CLI_USER_AGENT).toBe(`godaddy-cli/${packageJson.version}`);
  });

  test("cliTraceHeaders sets lowercase user-agent and x-request-id", () => {
    const h = cliTraceHeaders();
    expect(h["user-agent"]).toBe(CLI_USER_AGENT);
    const rid = h["x-request-id"];
    expect(rid).toBeDefined();
    // Standard UUID string forms are 32 hex digits (with optional hyphens);
    // avoid a strict 8-4-4-4-12 regex so a future uuid package output shape
    // does not flake the test as long as it remains 32 hex.
    const hex = rid.replace(/-/g, "");
    expect(hex).toHaveLength(32);
    expect(hex).toMatch(/^[0-9a-f]+$/i);
  });

  test("getRequestHeaders merges auth with trace headers", () => {
    const h = getRequestHeaders("tok");
    expect(h.Authorization).toBe("Bearer tok");
    expect(h["user-agent"]).toBe(CLI_USER_AGENT);
    expect(h["x-request-id"]).toBeTruthy();
  });
});
