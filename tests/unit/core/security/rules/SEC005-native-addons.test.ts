import { buildAliasMaps } from "@/core/security/alias-builder.ts";
import { scanFile } from "@/core/security/engine.ts";
import { SEC005 } from "@/core/security/rules/SEC005-native-addons.ts";
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

describe("SEC005: No native addons", () => {
	describe(".node file loading", () => {
		it("should detect require('*.node')", () => {
			const code = `const addon = require('./addon.node');`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC005],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
			expect(findings[0].ruleId).toBe("SEC005");
			expect(findings[0].severity).toBe("block");
		});

		it("should detect require with absolute .node path", () => {
			const code = `const addon = require('/path/to/native.node');`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC005],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
		});
	});

	describe("native binding libraries", () => {
		it("should detect node-gyp-build import", () => {
			const code = `import gyp from 'node-gyp-build';`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC005],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
			expect(findings[0].ruleId).toBe("SEC005");
		});

		it("should detect ffi-napi import", () => {
			const code = `import ffi from 'ffi-napi';`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC005],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
		});

		it("should detect ref-napi import", () => {
			const code = `import ref from 'ref-napi';`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC005],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
		});

		it("should detect require of native binding libs", () => {
			const code = `const gyp = require('node-gyp-build');`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC005],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(1);
		});
	});

	describe("safe code", () => {
		it("should not detect regular .js requires", () => {
			const code = `const lib = require('./lib.js');`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC005],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});

		it("should not detect JSON requires", () => {
			const code = `const config = require('./config.json');`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const findings = scanFile(
				"test.ts",
				code,
				[SEC005],
				mockConfig,
				aliasMaps,
			);

			expect(findings).toHaveLength(0);
		});
	});
});
