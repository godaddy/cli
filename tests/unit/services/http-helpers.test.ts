import { describe, expect, test } from "vitest";
import packageJson from "../../../package.json";
import {
  CLI_USER_AGENT,
  cliTraceHeaders,
  getRequestHeaders,
} from "../../../src/services/http-helpers";

describe("http-helpers trace headers", () => {
  test("CLI_USER_AGENT includes package version", () => {
    expect(CLI_USER_AGENT).toBe(`godaddy-cli/${packageJson.version}`);
  });

  test("cliTraceHeaders sets User-Agent and X-Request-ID", () => {
    const h = cliTraceHeaders();
    expect(h["User-Agent"]).toBe(CLI_USER_AGENT);
    expect(h["X-Request-ID"]).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i,
    );
  });

  test("getRequestHeaders merges auth with trace headers", () => {
    const h = getRequestHeaders("tok");
    expect(h.Authorization).toBe("Bearer tok");
    expect(h["User-Agent"]).toBe(CLI_USER_AGENT);
    expect(h["X-Request-ID"]).toBeTruthy();
  });
});
