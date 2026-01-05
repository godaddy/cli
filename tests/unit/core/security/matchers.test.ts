import { buildAliasMaps } from "@/core/security/alias-builder.ts";
import {
	getStringLiteralValue,
	isBufferFromCall,
	isCallToGlobal,
	isIdentifier,
	isImportOf,
	isMemberCall,
	isNewExpressionOf,
	isProcessProperty,
	isRequireOf,
	matchesSensitivePath,
	matchesUrl,
} from "@/core/security/matchers.ts";
import * as ts from "typescript";
import { describe, expect, it } from "vitest";

/**
 * Helper to create a TypeScript source file from code string
 */
function createSourceFile(code: string, fileName = "test.ts"): ts.SourceFile {
	return ts.createSourceFile(
		fileName,
		code,
		ts.ScriptTarget.Latest,
		true,
		ts.ScriptKind.TS,
	);
}

/**
 * Helper to find first node of specific kind in source file
 */
function findNode(
	sourceFile: ts.SourceFile,
	predicate: (node: ts.Node) => boolean,
): ts.Node | null {
	let found: ts.Node | null = null;

	function visit(node: ts.Node): void {
		if (found) return;
		if (predicate(node)) {
			found = node;
			return;
		}
		ts.forEachChild(node, visit);
	}

	visit(sourceFile);
	return found;
}

describe("Shared Matchers Library", () => {
	describe("isIdentifier", () => {
		it("should match identifier with correct name", () => {
			const code = "const eval = 5;";
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isIdentifier(n));

			expect(node).toBeDefined();
			expect(isIdentifier(node!, "eval")).toBe(true);
		});

		it("should not match identifier with different name", () => {
			const code = "const foo = 5;";
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isIdentifier(n));

			expect(isIdentifier(node!, "eval")).toBe(false);
		});

		it("should not match non-identifier nodes", () => {
			const code = "5";
			const sourceFile = createSourceFile(code);
			const node = findNode(
				sourceFile,
				(n) => n.kind === ts.SyntaxKind.NumericLiteral,
			);

			expect(isIdentifier(node!, "anything")).toBe(false);
		});
	});

	describe("isCallToGlobal", () => {
		it("should detect eval() calls", () => {
			const code = `eval("code");`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isCallExpression(n));

			expect(node).toBeDefined();
			expect(isCallToGlobal(node!, "eval")).toBe(true);
		});

		it("should detect require() calls", () => {
			const code = `require("module");`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isCallExpression(n));

			expect(isCallToGlobal(node!, "require")).toBe(true);
		});

		it("should not match method calls", () => {
			const code = `obj.eval("code");`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isCallExpression(n));

			expect(isCallToGlobal(node!, "eval")).toBe(false);
		});

		it("should not match calls to different globals", () => {
			const code = `eval("code");`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isCallExpression(n));

			expect(isCallToGlobal(node!, "require")).toBe(false);
		});

		it("should not match non-call nodes", () => {
			const code = "const x = 5;";
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isIdentifier(n));

			expect(isCallToGlobal(node!, "eval")).toBe(false);
		});
	});

	describe("isNewExpressionOf", () => {
		it("should detect new Function() expressions", () => {
			const code = `new Function("return 1");`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isNewExpression(n));

			expect(node).toBeDefined();
			expect(isNewExpressionOf(node!, "Function")).toBe(true);
		});

		it("should detect other new expressions", () => {
			const code = "new Date();";
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isNewExpression(n));

			expect(isNewExpressionOf(node!, "Date")).toBe(true);
		});

		it("should not match different constructors", () => {
			const code = "new Date();";
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isNewExpression(n));

			expect(isNewExpressionOf(node!, "Function")).toBe(false);
		});

		it("should not match non-new-expression nodes", () => {
			const code = `Function("return 1");`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isCallExpression(n));

			expect(isNewExpressionOf(node!, "Function")).toBe(false);
		});
	});

	describe("isMemberCall", () => {
		it("should detect member calls on aliased modules (ESM)", () => {
			const code = `
				import cp from 'child_process';
				cp.exec('ls');
			`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const node = findNode(sourceFile, (n) => ts.isCallExpression(n));

			expect(node).toBeDefined();
			expect(
				isMemberCall(node!, {
					objectIsAliasOf: "child_process",
					method: "exec",
					aliasMaps,
				}),
			).toBe(true);
		});

		it("should detect member calls on aliased modules (CJS)", () => {
			const code = `
		const cp = require('child_process');
		cp.spawn('node');
		`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			// Find the cp.spawn() call, not the require() call
			const node = findNode(
				sourceFile,
				(n) =>
					ts.isCallExpression(n) && ts.isPropertyAccessExpression(n.expression),
			);

			expect(node).toBeDefined();
			expect(
				isMemberCall(node!, {
					objectIsAliasOf: "child_process",
					method: "spawn",
					aliasMaps,
				}),
			).toBe(true);
		});

		it("should detect namespace member calls", () => {
			const code = `
				import * as VM from 'vm';
				VM.runInContext('code', {});
			`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const node = findNode(sourceFile, (n) => ts.isCallExpression(n));

			expect(
				isMemberCall(node!, {
					objectIsAliasOf: "vm",
					method: "runInContext",
					aliasMaps,
				}),
			).toBe(true);
		});

		it("should not match calls on non-aliased objects", () => {
			const code = `
				const obj = { exec: () => {} };
				obj.exec('ls');
			`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const node = findNode(sourceFile, (n) => ts.isCallExpression(n));

			expect(
				isMemberCall(node!, {
					objectIsAliasOf: "child_process",
					method: "exec",
					aliasMaps,
				}),
			).toBe(false);
		});

		it("should not match different method names", () => {
			const code = `
				import cp from 'child_process';
				cp.fork('script.js');
			`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const node = findNode(sourceFile, (n) => ts.isCallExpression(n));

			expect(
				isMemberCall(node!, {
					objectIsAliasOf: "child_process",
					method: "exec",
					aliasMaps,
				}),
			).toBe(false);
		});

		it("should not match non-call expressions", () => {
			const code = `
				import cp from 'child_process';
				cp.exec;
			`;
			const sourceFile = createSourceFile(code);
			const aliasMaps = buildAliasMaps(sourceFile);
			const node = findNode(
				sourceFile,
				(n) =>
					ts.isPropertyAccessExpression(n) &&
					!ts.isCallExpression(n.parent as ts.Node),
			);

			expect(
				isMemberCall(node!, {
					objectIsAliasOf: "child_process",
					method: "exec",
					aliasMaps,
				}),
			).toBe(false);
		});
	});

	describe("isProcessProperty", () => {
		it("should detect process.binding", () => {
			const code = `process.binding('natives');`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) =>
				ts.isPropertyAccessExpression(n),
			);

			expect(node).toBeDefined();
			expect(isProcessProperty(node!, "binding")).toBe(true);
		});

		it("should detect process.dlopen", () => {
			const code = `process.dlopen(module, 'addon.node');`;
			const sourceFile = createSourceFile(code);
			const node = findNode(
				sourceFile,
				(n) =>
					ts.isPropertyAccessExpression(n) &&
					ts.isIdentifier(n.name) &&
					n.name.text === "dlopen",
			);

			expect(isProcessProperty(node!, "dlopen")).toBe(true);
		});

		it("should not match other process properties", () => {
			const code = "process.env.NODE_ENV;";
			const sourceFile = createSourceFile(code);
			const node = findNode(
				sourceFile,
				(n) =>
					ts.isPropertyAccessExpression(n) &&
					ts.isIdentifier(n.name) &&
					n.name.text === "env",
			);

			expect(isProcessProperty(node!, "binding")).toBe(false);
		});

		it("should not match non-process objects", () => {
			const code = "obj.binding();";
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) =>
				ts.isPropertyAccessExpression(n),
			);

			expect(isProcessProperty(node!, "binding")).toBe(false);
		});
	});

	describe("isRequireOf", () => {
		it("should match require with pattern", () => {
			const code = `require('inspector');`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isCallExpression(n));

			expect(node).toBeDefined();
			expect(isRequireOf(node!, /^inspector$/)).toBe(true);
		});

		it("should match .node file requires", () => {
			const code = `require('./addon.node');`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isCallExpression(n));

			expect(isRequireOf(node!, /\.node$/)).toBe(true);
		});

		it("should not match different modules", () => {
			const code = `require('fs');`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isCallExpression(n));

			expect(isRequireOf(node!, /^inspector$/)).toBe(false);
		});

		it("should not match non-require calls", () => {
			const code = `eval('inspector');`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isCallExpression(n));

			expect(isRequireOf(node!, /^inspector$/)).toBe(false);
		});
	});

	describe("isImportOf", () => {
		it("should detect imports of specific module", () => {
			const code = `import { open } from 'inspector';`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isImportDeclaration(n));

			expect(node).toBeDefined();
			expect(isImportOf(node!, "inspector")).toBe(true);
		});

		it("should detect namespace imports", () => {
			const code = `import * as insp from 'inspector';`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isImportDeclaration(n));

			expect(isImportOf(node!, "inspector")).toBe(true);
		});

		it("should not match different modules", () => {
			const code = `import fs from 'fs';`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isImportDeclaration(n));

			expect(isImportOf(node!, "inspector")).toBe(false);
		});

		it("should not match non-import nodes", () => {
			const code = "const x = 5;";
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isVariableStatement(n));

			expect(isImportOf(node!, "inspector")).toBe(false);
		});
	});

	describe("getStringLiteralValue", () => {
		it("should extract value from string literals", () => {
			const code = `"https://example.com"`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isStringLiteral(n));

			expect(node).toBeDefined();
			expect(getStringLiteralValue(node!)).toBe("https://example.com");
		});

		it("should extract value from simple template literals", () => {
			const code = "`https://example.com`";
			const sourceFile = createSourceFile(code);
			const node = findNode(
				sourceFile,
				(n) => n.kind === ts.SyntaxKind.NoSubstitutionTemplateLiteral,
			);

			expect(node).toBeDefined();
			expect(getStringLiteralValue(node!)).toBe("https://example.com");
		});

		it("should return null for template literals with substitutions", () => {
			const code = "`https://${domain}.com`";
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isTemplateExpression(n));

			expect(getStringLiteralValue(node!)).toBeNull();
		});

		it("should return null for non-string nodes", () => {
			const code = "123";
			const sourceFile = createSourceFile(code);
			const node = findNode(
				sourceFile,
				(n) => n.kind === ts.SyntaxKind.NumericLiteral,
			);

			expect(getStringLiteralValue(node!)).toBeNull();
		});
	});

	describe("isBufferFromCall", () => {
		it("should detect Buffer.from calls", () => {
			const code = `Buffer.from('data', 'base64');`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isCallExpression(n));

			expect(node).toBeDefined();
			expect(isBufferFromCall(node!)).toBe(true);
		});

		it("should detect Buffer.from with base64 encoding", () => {
			const code = `Buffer.from('data', 'base64');`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isCallExpression(n));

			expect(isBufferFromCall(node!, "base64")).toBe(true);
		});

		it("should detect Buffer.from with hex encoding", () => {
			const code = `Buffer.from('data', 'hex');`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isCallExpression(n));

			expect(isBufferFromCall(node!, "hex")).toBe(true);
		});

		it("should not match when encoding doesn't match", () => {
			const code = `Buffer.from('data', 'utf8');`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isCallExpression(n));

			expect(isBufferFromCall(node!, "base64")).toBe(false);
		});

		it("should not match other Buffer methods", () => {
			const code = "Buffer.alloc(10);";
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isCallExpression(n));

			expect(isBufferFromCall(node!)).toBe(false);
		});

		it("should not match non-Buffer calls", () => {
			const code = `obj.from('data');`;
			const sourceFile = createSourceFile(code);
			const node = findNode(sourceFile, (n) => ts.isCallExpression(n));

			expect(isBufferFromCall(node!)).toBe(false);
		});
	});

	describe("matchesUrl", () => {
		it("should match https URLs", () => {
			expect(matchesUrl("https://example.com")).toBe(true);
		});

		it("should match http URLs", () => {
			expect(matchesUrl("http://example.com")).toBe(true);
		});

		it("should match URLs with paths", () => {
			expect(matchesUrl("https://api.example.com/v1/data")).toBe(true);
		});

		it("should match URLs embedded in strings", () => {
			expect(matchesUrl("Visit https://example.com for more")).toBe(true);
		});

		it("should not match file URLs", () => {
			expect(matchesUrl("file:///etc/passwd")).toBe(false);
		});

		it("should not match non-URL strings", () => {
			expect(matchesUrl("example.com")).toBe(false);
			expect(matchesUrl("just some text")).toBe(false);
		});
	});

	describe("matchesSensitivePath", () => {
		it("should match ~/.ssh paths", () => {
			expect(matchesSensitivePath("~/.ssh/id_rsa")).toBe(true);
		});

		it("should match /etc/passwd", () => {
			expect(matchesSensitivePath("/etc/passwd")).toBe(true);
		});

		it("should match /var/run/secrets paths", () => {
			expect(matchesSensitivePath("/var/run/secrets/token")).toBe(true);
		});

		it("should match paths containing sensitive patterns", () => {
			expect(matchesSensitivePath("cat /etc/passwd")).toBe(true);
		});

		it("should not match normal paths", () => {
			expect(matchesSensitivePath("./config.json")).toBe(false);
			expect(matchesSensitivePath("/home/user/file.txt")).toBe(false);
		});

		it("should not match non-path strings", () => {
			expect(matchesSensitivePath("regular string")).toBe(false);
		});
	});
});
