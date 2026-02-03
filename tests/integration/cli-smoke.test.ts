import { execSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const CLI_PATH = join(process.cwd(), "dist", "cli.js");

function isKeytarAvailable(): boolean {
	try {
		require.resolve("keytar");
		const keytarPath = join(
			process.cwd(),
			"node_modules/.pnpm/keytar@7.9.0/node_modules/keytar/build/Release/keytar.node",
		);
		return existsSync(keytarPath);
	} catch {
		return false;
	}
}

function isBuildAvailable(): boolean {
	if (!existsSync(CLI_PATH)) {
		try {
			execSync("pnpm run build", { stdio: "pipe" });
		} catch {
			return false;
		}
	}
	return existsSync(CLI_PATH);
}

const keytarAvailable = isKeytarAvailable();
const buildAvailable = isBuildAvailable();
const canRunCli = keytarAvailable && buildAvailable;

describe("CLI Smoke Tests", () => {
	describe("--help", () => {
		it.skipIf(!canRunCli)("should display help and exit with code 0", () => {
			const result = execSync(`node ${CLI_PATH} --help`, {
				encoding: "utf-8",
			});

			expect(result).toContain("GoDaddy");
			expect(result).toContain("application");
			expect(result).toContain("auth");
			expect(result).toContain("env");
		});

		it.skipIf(!canRunCli)("should display subcommand help", () => {
			const result = execSync(`node ${CLI_PATH} application --help`, {
				encoding: "utf-8",
			});

			expect(result).toContain("info");
			expect(result).toContain("deploy");
			expect(result).toContain("release");
		});
	});

	describe("--version", () => {
		it.skipIf(!canRunCli)("should display version and exit with code 0", () => {
			const result = execSync(`node ${CLI_PATH} --version`, {
				encoding: "utf-8",
			});

			expect(result.trim()).toMatch(/^\d+\.\d+\.\d+$/);
		});
	});

	describe("invalid environment", () => {
		it.skipIf(!canRunCli)(
			"should exit with error for invalid --env value",
			() => {
				expect(() => {
					execSync(`node ${CLI_PATH} --env invalid-env env get`, {
						encoding: "utf-8",
						stdio: "pipe",
					});
				}).toThrow();
			},
		);
	});

	describe("unknown command", () => {
		it.skipIf(!canRunCli)("should show error for unknown command", () => {
			expect(() => {
				execSync(`node ${CLI_PATH} nonexistent-command`, {
					encoding: "utf-8",
					stdio: "pipe",
				});
			}).toThrow();
		});
	});
});
