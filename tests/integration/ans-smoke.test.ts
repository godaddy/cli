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

describe("ANS command smoke tests", () => {
  beforeAll(() => {
    if (!existsSync(CLI_PATH)) {
      execSync("pnpm run build", { stdio: "inherit" });
    }
  });

  it("discovery tree includes ans group", () => {
    const result = runCli([]);
    expect(result.status).toBe(0);

    const payload = JSON.parse(result.stdout);
    const tree = payload.result.command_tree;
    const ansGroup = tree.children.find(
      (c: { id: string }) => c.id === "ans.group",
    );
    expect(ansGroup).toBeDefined();
    expect(ansGroup.command).toBe("godaddy ans");
  });

  it("ans command tree includes all 14 subcommands", () => {
    const result = runCli([]);
    const payload = JSON.parse(result.stdout);
    const tree = payload.result.command_tree;
    const ansGroup = tree.children.find(
      (c: { id: string }) => c.id === "ans.group",
    );

    const ids = (ansGroup.children as Array<{ id: string }>).map((c) => c.id);
    const expected = [
      "ans.register",
      "ans.status",
      "ans.verify-acme",
      "ans.verify-dns",
      "ans.submit-server-csr",
      "ans.submit-identity-csr",
      "ans.csr-status",
      "ans.revoke",
      "ans.search",
      "ans.resolve",
      "ans.events",
      "ans.get-server-certs",
      "ans.get-identity-certs",
      "ans.badge",
    ];
    for (const id of expected) {
      expect(ids).toContain(id);
    }
  });

  it("ans bare command returns discovery envelope", () => {
    const result = runCli(["ans"]);
    expect(result.status).toBe(0);

    const payload = JSON.parse(result.stdout);
    expect(payload.ok).toBe(true);
    expect(payload.command).toBe("godaddy ans");
    expect(Array.isArray(payload.result.commands)).toBe(true);
    expect(payload.result.commands).toContain("register");
  });

  it("ans register without credentials returns auth error envelope", () => {
    const result = spawnSync(
      "node",
      [
        CLI_PATH,
        "ans",
        "register",
        "--host",
        "example.ai",
        "--a2a-url",
        "https://example.ai/a2a",
        "--server-csr-file",
        "/nonexistent.pem",
        "--identity-csr-file",
        "/nonexistent.pem",
      ],
      {
        encoding: "utf-8",
        env: {
          ...process.env,
          GODADDY_KEY: undefined,
          GODADDY_SECRET: undefined,
        },
      },
    );

    // Either a config error (missing creds) or a file error (csr file not found)
    // — either way the CLI exits with a non-zero code and emits an error envelope
    const payload = JSON.parse(result.stdout);
    expect(payload.ok).toBe(false);
    expect(typeof payload.error.message).toBe("string");
    expect(typeof payload.error.code).toBe("string");
  });

  it("ans revoke rejects unknown reason with validation error", () => {
    const result = spawnSync(
      "node",
      [CLI_PATH, "ans", "revoke", "fake-agent-id", "--reason", "NOPE"],
      {
        encoding: "utf-8",
        env: {
          ...process.env,
          GODADDY_KEY: "test",
          GODADDY_SECRET: "test",
        },
      },
    );

    const payload = JSON.parse(result.stdout);
    expect(payload.ok).toBe(false);
    expect(payload.error.code).toBe("VALIDATION_ERROR");
    expect(payload.error.message).toContain("NOPE");
  });
});
