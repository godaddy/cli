import { execSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { beforeAll, describe, expect, it } from "vitest";

const CLI_PATH = join(process.cwd(), "dist", "cli.js");

describe("CLI Smoke Tests", () => {
	beforeAll(() => {
		if (!existsSync(CLI_PATH)) {
			execSync("pnpm run build", { stdio: "inherit" });
		}
	});

	describe("--help", () => {
		it("should display help and exit with code 0", () => {
			const result = execSync(`node ${CLI_PATH} --help`, {
				encoding: "utf-8",
			});

			expect(result).toContain("GoDaddy");
			expect(result).toContain("application");
			expect(result).toContain("auth");
			expect(result).toContain("env");
		});

		it("should display subcommand help", () => {
			const result = execSync(`node ${CLI_PATH} application --help`, {
				encoding: "utf-8",
			});

			expect(result).toContain("info");
			expect(result).toContain("deploy");
			expect(result).toContain("release");
		});
	});

	describe("--version", () => {
		it("should display version and exit with code 0", () => {
			const result = execSync(`node ${CLI_PATH} --version`, {
				encoding: "utf-8",
			});

			expect(result.trim()).toMatch(/^\d+\.\d+\.\d+$/);
		});
	});

	describe("invalid environment", () => {
		it("should exit with error for invalid --env value", () => {
			expect(() => {
				execSync(`node ${CLI_PATH} --env invalid-env env get`, {
					encoding: "utf-8",
					stdio: "pipe",
				});
			}).toThrow();
		});
	});

	describe("unknown command", () => {
		it("should show error for unknown command", () => {
			expect(() => {
				execSync(`node ${CLI_PATH} nonexistent-command`, {
					encoding: "utf-8",
					stdio: "pipe",
				});
			}).toThrow();
		});
	});
});
