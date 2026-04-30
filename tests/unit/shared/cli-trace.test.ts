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
    expect(h["x-request-id"]).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i,
    );
  });

  test("getRequestHeaders merges auth with trace headers", () => {
    const h = getRequestHeaders("tok");
    expect(h.Authorization).toBe("Bearer tok");
    expect(h["user-agent"]).toBe(CLI_USER_AGENT);
    expect(h["x-request-id"]).toBeTruthy();
  });
});
