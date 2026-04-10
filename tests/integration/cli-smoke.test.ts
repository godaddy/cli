import { execSync, spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { beforeAll, describe, expect, it } from "vitest";

const CLI_PATH = join(process.cwd(), "dist", "cli.js");

function runCli(args: string[]) {
  const result = spawnSync("node", [CLI_PATH, ...args], {
    encoding: "utf-8",
  });
  return {
    stdout: result.stdout.trim(),
    stderr: result.stderr.trim(),
    status: result.status ?? 0,
  };
}

describe("CLI Smoke Tests", () => {
  beforeAll(() => {
    if (!existsSync(CLI_PATH)) {
      execSync("pnpm run build", { stdio: "inherit" });
    }
  });

  it("root command returns JSON discovery envelope", () => {
    const result = runCli([]);
    expect(result.status).toBe(0);

    const payload = JSON.parse(result.stdout);
    expect(payload.ok).toBe(true);
    expect(payload.command).toBe("godaddy");
    expect(payload.result.command_tree.command).toBe("godaddy");
    expect(Array.isArray(payload.next_actions)).toBe(true);
  });

  it("discovery tree includes --follow on application deploy", () => {
    const result = runCli([]);
    expect(result.status).toBe(0);

    const payload = JSON.parse(result.stdout) as {
      result: { command_tree: { children?: Array<Record<string, unknown>> } };
    };
    const rootChildren = payload.result.command_tree.children ?? [];
    const applicationNode = rootChildren.find(
      (node) => node.id === "application.group",
    ) as { children?: Array<Record<string, unknown>> } | undefined;
    const applicationChildren = applicationNode?.children ?? [];
    const deployNode = applicationChildren.find(
      (node) => node.id === "application.deploy",
    );

    expect(deployNode).toBeDefined();
    expect(String(deployNode?.command)).toContain("[--follow]");
    expect(String(deployNode?.usage)).toContain("[--follow]");
  });

  it("--pretty formats success envelopes with indentation", () => {
    const result = runCli(["--pretty"]);
    expect(result.status).toBe(0);
    expect(result.stdout).toContain('\n  "ok": true');
    expect(result.stdout).toContain('\n  "result": {');

    const payload = JSON.parse(result.stdout);
    expect(payload.ok).toBe(true);
  });

  it("--env overrides active environment for command execution", () => {
    const result = runCli(["--env", "prod", "env", "get"]);
    expect(result.status).toBe(0);

    const payload = JSON.parse(result.stdout) as {
      ok: boolean;
      result: { environment: string };
    };
    expect(payload.ok).toBe(true);
    expect(payload.result.environment).toBe("prod");
  });

  it("application parent command returns subtree discovery envelope", () => {
    const result = runCli(["application"]);
    expect(result.status).toBe(0);

    const payload = JSON.parse(result.stdout);
    expect(payload.ok).toBe(true);
    expect(payload.result.command).toBe("godaddy application");
    expect(Array.isArray(payload.result.commands)).toBe(true);
  });

  it("api list returns catalog domains", () => {
    const result = runCli(["api", "list"]);
    expect(result.status).toBe(0);

    const payload = JSON.parse(result.stdout) as {
      ok: boolean;
      command: string;
      result: { domains: Array<{ name: string }> };
    };
    expect(payload.ok).toBe(true);
    expect(payload.command).toBe("godaddy api list");
    const expectedDomains = [
      "location-addresses",
      "catalog-products",
      "orders",
      "stores",
      "fulfillments",
      "metafields",
      "transactions",
      "businesses",
      "bulk-operations",
      "channels",
      "onboarding",
    ];

    for (const expectedDomain of expectedDomains) {
      expect(
        payload.result.domains.some((domain) => domain.name === expectedDomain),
      ).toBe(true);
    }
  });

  it("api describe returns endpoint details", () => {
    const result = runCli([
      "api",
      "describe",
      "/location/address-verifications",
    ]);
    expect(result.status).toBe(0);

    const payload = JSON.parse(result.stdout) as {
      ok: boolean;
      command: string;
      result: { operationId: string; method: string; path: string };
      next_actions: Array<{
        command: string;
        params?: Record<string, { value?: string }>;
      }>;
    };
    expect(payload.ok).toBe(true);
    expect(payload.command).toBe("godaddy api describe");
    expect(payload.result.operationId).toBe("commerce.location.verify-address");
    expect(payload.result.method).toBe("POST");
    expect(payload.result.path).toBe("/location/address-verifications");
    expect(payload.next_actions[0]?.command).toBe(
      "godaddy api call <endpoint>",
    );
    expect(payload.next_actions[0]?.params?.endpoint?.value).toBe(
      "/v1/commerce/location/address-verifications",
    );
  });

  it("api describe matches templated catalog paths", () => {
    const result = runCli([
      "api",
      "describe",
      "/stores/123e4567-e89b-12d3-a456-426614174000/catalog-subgraph",
    ]);
    expect(result.status).toBe(0);

    const payload = JSON.parse(result.stdout) as {
      ok: boolean;
      command: string;
      result: { operationId: string; method: string; path: string };
    };

    expect(payload.ok).toBe(true);
    expect(payload.command).toBe("godaddy api describe");
    expect(payload.result.operationId).toBe("postCatalogGraphql");
    expect(payload.result.method).toBe("POST");
    expect(payload.result.path).toBe("/stores/{storeId}/catalog-subgraph");
  });

  it("api search returns matching endpoints", () => {
    const result = runCli(["api", "search", "address"]);
    expect(result.status).toBe(0);

    const payload = JSON.parse(result.stdout) as {
      ok: boolean;
      command: string;
      result: { results: Array<{ operationId: string }> };
      next_actions: Array<{
        command: string;
        params?: Record<string, { value?: string }>;
      }>;
    };
    expect(payload.ok).toBe(true);
    expect(payload.command).toBe("godaddy api search");
    expect(
      payload.result.results.some(
        (item) => item.operationId === "commerce.location.verify-address",
      ),
    ).toBe(true);
    expect(payload.next_actions[0]?.command).toBe(
      "godaddy api describe <endpoint>",
    );
  });

  it("legacy api endpoint syntax routes to api call", () => {
    const result = runCli(["api", "/v1/commerce/location/addresses", "--help"]);
    expect(result.status).toBe(0);
    expect(result.stdout).toContain(
      "Make authenticated requests to the GoDaddy API",
    );
    expect(result.stdout).toContain("<endpoint>");
  });

  it("api call rejects untrusted absolute URLs", () => {
    const result = runCli(["api", "call", "https://example.com/v1/domains"]);
    expect(result.status).toBe(1);

    const payload = JSON.parse(result.stdout) as {
      ok: boolean;
      error: { code: string; message: string };
      fix: string;
    };

    expect(payload.ok).toBe(false);
    expect(payload.error.code).toBe("VALIDATION_ERROR");
    expect(payload.error.message).toContain("trusted GoDaddy API URL");
  });

  it("unknown command returns structured error envelope", () => {
    const result = runCli(["nonexistent-command"]);
    expect(result.status).toBe(1);

    const payload = JSON.parse(result.stdout);
    expect(payload.ok).toBe(false);
    expect(payload.error.code).toBe("COMMAND_NOT_FOUND");
    expect(payload.fix).toContain("godaddy");
  });

  it("--pretty formats structured error envelopes with indentation", () => {
    const result = runCli(["--pretty", "nonexistent-command"]);
    expect(result.status).toBe(1);
    expect(result.stdout).toContain('\n  "ok": false');
    expect(result.stdout).toContain('\n  "error": {');

    const payload = JSON.parse(result.stdout);
    expect(payload.error.code).toBe("COMMAND_NOT_FOUND");
  });

  it("unsupported --output option returns structured error envelope", () => {
    const result = runCli(["env", "get", "--output", "json"]);
    expect(result.status).toBe(1);

    const payload = JSON.parse(result.stdout);
    expect(payload.ok).toBe(false);
    expect(payload.error.code).toBe("UNSUPPORTED_OPTION");
  });

  it("application info requires <name> at parse-time", () => {
    const result = runCli(["application", "info"]);
    expect(result.status).toBe(1);

    const payload = JSON.parse(result.stdout);
    expect(payload.ok).toBe(false);
    expect(payload.error.code).toBe("VALIDATION_ERROR");
    expect(payload.error.message).toContain("Missing argument <name>");
  });

  it("application validate requires <name> at parse-time", () => {
    const result = runCli(["application", "validate"]);
    expect(result.status).toBe(1);

    const payload = JSON.parse(result.stdout);
    expect(payload.ok).toBe(false);
    expect(payload.error.code).toBe("VALIDATION_ERROR");
    expect(payload.error.message).toContain("Missing argument <name>");
  });

  it("deploy --follow emits start before terminal error on preflight failure", () => {
    const result = runCli([
      "application",
      "deploy",
      "demo",
      "--follow",
      "--environment",
      "invalid",
    ]);
    expect(result.status).toBe(1);

    const lines = result.stdout.split("\n").filter((line) => line.length > 0);
    expect(lines.length).toBeGreaterThanOrEqual(2);

    const firstEvent = JSON.parse(lines[0]) as { type: string };
    const lastEvent = JSON.parse(lines[lines.length - 1]) as {
      type: string;
      error?: { code: string };
    };

    expect(firstEvent.type).toBe("start");
    expect(lastEvent.type).toBe("error");
    expect(lastEvent.error?.code).toBe("VALIDATION_ERROR");
  });

  it("--help still prints framework help text", () => {
    const result = runCli(["--help"]);
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("COMMANDS");
    expect(result.stdout).toContain("application");
  });

  it("-v enables basic verbose mode", () => {
    const result = runCli(["-v"]);
    expect(result.status).toBe(0);

    const payload = JSON.parse(result.stdout);
    expect(payload.ok).toBe(true);
    expect(payload.command).toBe("godaddy -v");
    expect(result.stderr).toContain("(verbose output enabled)");
    expect(result.stderr).not.toContain("full details");
  });

  it("-vv enables full verbose mode", () => {
    const result = runCli(["-vv"]);
    expect(result.status).toBe(0);

    const payload = JSON.parse(result.stdout);
    expect(payload.ok).toBe(true);
    expect(payload.command).toBe("godaddy -vv");
    expect(result.stderr).toContain("(verbose output enabled: full details)");
  });

  it("repeated -v flags enable full verbose mode", () => {
    const result = runCli(["-v", "-v"]);
    expect(result.status).toBe(0);

    const payload = JSON.parse(result.stdout);
    expect(payload.ok).toBe(true);
    expect(payload.command).toBe("godaddy -v -v");
    expect(result.stderr).toContain("(verbose output enabled: full details)");
  });

  it("--info aliases to basic verbose mode", () => {
    const result = runCli(["--info"]);
    expect(result.status).toBe(0);

    const payload = JSON.parse(result.stdout);
    expect(payload.ok).toBe(true);
    expect(payload.command).toBe("godaddy --info");
    expect(result.stderr).toContain("(verbose output enabled)");
    expect(result.stderr).not.toContain("full details");
  });

  it("--debug aliases to full verbose mode", () => {
    const result = runCli(["--debug"]);
    expect(result.status).toBe(0);

    const payload = JSON.parse(result.stdout);
    expect(payload.ok).toBe(true);
    expect(payload.command).toBe("godaddy --debug");
    expect(result.stderr).toContain("(verbose output enabled: full details)");
  });

  it("--log-level error produces no verbose output", () => {
    const result = runCli(["--log-level", "error", "env", "get"]);
    expect(result.status).toBe(0);

    const payload = JSON.parse(result.stdout);
    expect(payload.ok).toBe(true);
    expect(result.stderr).not.toContain("verbose output enabled");
  });

  it("--log-level info enables basic verbose mode", () => {
    const result = runCli(["--log-level", "info", "env", "get"]);
    expect(result.status).toBe(0);

    const payload = JSON.parse(result.stdout);
    expect(payload.ok).toBe(true);
    expect(result.stderr).toContain("(verbose output enabled)");
    expect(result.stderr).not.toContain("full details");
  });

  it("--log-level debug enables full verbose mode", () => {
    const result = runCli(["--log-level", "debug", "env", "get"]);
    expect(result.status).toBe(0);

    const payload = JSON.parse(result.stdout);
    expect(payload.ok).toBe(true);
    expect(result.stderr).toContain("(verbose output enabled: full details)");
  });

  it("--log-level is case-insensitive", () => {
    const result = runCli(["--log-level", "DEBUG", "env", "get"]);
    expect(result.status).toBe(0);

    expect(result.stderr).toContain("(verbose output enabled: full details)");
  });

  it("unknown --log-level emits a warning", () => {
    const result = runCli(["--log-level", "bogus", "env", "get"]);
    expect(result.status).toBe(0);

    expect(result.stderr).toContain('unknown --log-level "bogus"');
    expect(result.stderr).toContain("error, info, debug");
  });

  it("--log-level combined with --verbose emits conflict warning", () => {
    const result = runCli(["--log-level", "debug", "--verbose", "env", "get"]);
    expect(result.status).toBe(0);

    expect(result.stderr).toContain(
      "--log-level overrides --verbose/--debug/--info flags",
    );
    expect(result.stderr).toContain("(verbose output enabled: full details)");
  });

  it("--log-level combined with --debug emits conflict warning", () => {
    const result = runCli(["--log-level", "info", "--debug", "env", "get"]);
    expect(result.status).toBe(0);

    expect(result.stderr).toContain(
      "--log-level overrides --verbose/--debug/--info flags",
    );
    // --log-level info wins over --debug
    expect(result.stderr).toContain("(verbose output enabled)");
    expect(result.stderr).not.toContain("full details)");
  });

  it("--log-level is stripped from argv and does not break subcommands", () => {
    const result = runCli(["--log-level", "error", "application"]);
    expect(result.status).toBe(0);

    const payload = JSON.parse(result.stdout);
    expect(payload.ok).toBe(true);
    expect(payload.result.command).toBe("godaddy application");
  });
});
