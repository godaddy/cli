import { buildAliasMaps } from "@/core/security/alias-builder.ts";
import { scanFile } from "@/core/security/engine.ts";
import { SEC010 } from "@/core/security/rules/SEC010-sensitive-paths.ts";
import type { SecurityConfig } from "@/core/security/types.ts";
import ts from "typescript";
import { describe, expect, it } from "vitest";

function createSourceFile(code: string): ts.SourceFile {
	return ts.createSourceFile(
		"test.ts",
		code,
		ts.ScriptTarget.Latest,
		true,
		ts.ScriptKind.TS,
	);
}

const mockConfig: SecurityConfig = {
	mode: "strict",
	trustedDomains: ["*.godaddy.com"],
	exclude: [],
};

describe("SEC010: Sensitive file path detection", () => {
	describe("SSH paths", () => {
		it("should warn on ~/.ssh/id_rsa", () => {
			const code = `const keyPath = "~/.ssh/id_rsa";`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC010],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
			expect(findings[0].ruleId).toBe("SEC010");
			expect(findings[0].severity).toBe("warn");
		});

		it("should warn on ~/.ssh directory", () => {
			const code = `const sshDir = "~/.ssh";`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC010],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
		});
	});

	describe("system files", () => {
		it("should warn on /etc/passwd", () => {
			const code = `const passwd = "/etc/passwd";`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC010],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
			expect(findings[0].ruleId).toBe("SEC010");
		});

		it("should warn on /etc/shadow", () => {
			const code = `fs.readFileSync("/etc/shadow");`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC010],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
		});
	});

	describe("Kubernetes secrets", () => {
		it("should warn on /var/run/secrets path", () => {
			const code = `const token = "/var/run/secrets/kubernetes.io/serviceaccount/token";`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC010],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
			expect(findings[0].ruleId).toBe("SEC010");
		});
	});

	describe("safe paths", () => {
		it("should not warn on regular file paths", () => {
			const code = `const path = "/home/user/data.json";`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC010],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});

		it("should not warn on relative paths", () => {
			const code = `const path = "./config/settings.json";`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC010],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});

		it("should not warn on node_modules paths", () => {
			const code = `const modulePath = "/node_modules/package/index.js";`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC010],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});
	});

	describe("multiple sensitive paths", () => {
		it("should detect multiple violations", () => {
			const code = `
        const ssh = "~/.ssh/id_rsa";
        const passwd = "/etc/passwd";
        const secrets = "/var/run/secrets/token";
      `;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC010],
				mockConfig,
				aliasMaps,
			);

			expect(findings.length).toBe(3);
			expect(findings.every((f) => f.ruleId === "SEC010")).toBe(true);
		});
	});
});
