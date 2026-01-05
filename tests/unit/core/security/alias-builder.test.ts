import { buildAliasMaps, isAliasOf } from "@/core/security/alias-builder.ts";
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

describe("Alias Map Builder", () => {
	describe("buildAliasMaps", () => {
		describe("ESM Imports", () => {
			it("should track default imports", () => {
				const code = `import fs from 'fs';`;
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				expect(maps.moduleAliases.get("fs")).toContain("fs");
				expect(maps.namespaceAliases.size).toBe(0);
				expect(maps.namedImports.size).toBe(0);
			});

			it("should track namespace imports", () => {
				const code = `import * as VM from 'vm';`;
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				expect(maps.namespaceAliases.get("vm")).toBe("VM");
				expect(maps.moduleAliases.size).toBe(0);
				expect(maps.namedImports.size).toBe(0);
			});

			it("should track named imports", () => {
				const code = `import { exec, spawn } from 'child_process';`;
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				const namedMap = maps.namedImports.get("child_process");
				expect(namedMap?.get("exec")).toBe("exec");
				expect(namedMap?.get("spawn")).toBe("spawn");
				expect(maps.moduleAliases.size).toBe(0);
				expect(maps.namespaceAliases.size).toBe(0);
			});

			it("should track renamed named imports", () => {
				const code = `import { exec as execute, spawn as sp } from 'child_process';`;
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				const namedMap = maps.namedImports.get("child_process");
				expect(namedMap?.get("exec")).toBe("execute");
				expect(namedMap?.get("spawn")).toBe("sp");
			});

			it("should track multiple imports from same module", () => {
				const code = `
					import fs from 'fs';
					import { readFileSync, writeFileSync as write } from 'fs';
				`;
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				expect(maps.moduleAliases.get("fs")).toContain("fs");
				const namedMap = maps.namedImports.get("fs");
				expect(namedMap?.get("readFileSync")).toBe("readFileSync");
				expect(namedMap?.get("writeFileSync")).toBe("write");
			});
		});

		describe("CommonJS Requires", () => {
			it("should track direct require assignments", () => {
				const code = `const cp = require('child_process');`;
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				expect(maps.moduleAliases.get("child_process")).toContain("cp");
			});

			it("should track destructured requires", () => {
				const code = `const { exec, spawn } = require('child_process');`;
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				const namedMap = maps.namedImports.get("child_process");
				expect(namedMap?.get("exec")).toBe("exec");
				expect(namedMap?.get("spawn")).toBe("spawn");
			});

			it("should track renamed destructured requires", () => {
				const code = `const { exec: execute, spawn: sp } = require('child_process');`;
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				const namedMap = maps.namedImports.get("child_process");
				expect(namedMap?.get("exec")).toBe("execute");
				expect(namedMap?.get("spawn")).toBe("sp");
			});

			it("should track string literal property names", () => {
				const code = `const { "exec": execute } = require('child_process');`;
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				const namedMap = maps.namedImports.get("child_process");
				expect(namedMap?.get("exec")).toBe("execute");
			});

			it("should track computed property names with static strings", () => {
				const code = `const { ["exec"]: execute } = require('child_process');`;
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				const namedMap = maps.namedImports.get("child_process");
				expect(namedMap?.get("exec")).toBe("execute");
			});

			it("should track template literal property names without substitutions", () => {
				const code = "const { [`exec`]: execute } = require('child_process');";
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				const namedMap = maps.namedImports.get("child_process");
				expect(namedMap?.get("exec")).toBe("execute");
			});

			it("should ignore dynamic computed property names", () => {
				const code = `
					const key = 'exec';
					const { [key]: execute } = require('child_process');
				`;
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				const namedMap = maps.namedImports.get("child_process");
				expect(namedMap?.size ?? 0).toBe(0);
			});

			it("should track mixed require patterns", () => {
				const code = `
					const fs = require('fs');
					const { readFileSync } = require('fs');
				`;
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				expect(maps.moduleAliases.get("fs")).toContain("fs");
				const namedMap = maps.namedImports.get("fs");
				expect(namedMap?.get("readFileSync")).toBe("readFileSync");
			});
		});

		describe("Dynamic Imports", () => {
			it("should track dynamic imports with string literals", () => {
				const code = `const mod = import('crypto');`;
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				// Dynamic imports tracked as module aliases
				expect(maps.moduleAliases.get("crypto")).toBeDefined();
			});

			it("should ignore dynamic imports with variables", () => {
				const code = `
					const moduleName = 'crypto';
					const mod = import(moduleName);
				`;
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				// Should not track variable-based dynamic imports
				expect(maps.moduleAliases.get("crypto")).toBeUndefined();
			});
		});

		describe("Mixed Import Styles", () => {
			it("should handle ESM and CJS in same file", () => {
				const code = `
					import fs from 'fs';
					const cp = require('child_process');
					import * as path from 'path';
					const { promisify } = require('util');
				`;
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				expect(maps.moduleAliases.get("fs")).toContain("fs");
				expect(maps.moduleAliases.get("child_process")).toContain("cp");
				expect(maps.namespaceAliases.get("path")).toBe("path");
				expect(maps.namedImports.get("util")?.get("promisify")).toBe(
					"promisify",
				);
			});

			it("should track multiple aliases for same module", () => {
				const code = `
					import fs from 'fs';
					const fileSystem = require('fs');
					import * as FS from 'fs';
				`;
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				const aliases = maps.moduleAliases.get("fs");
				expect(aliases).toContain("fs");
				expect(aliases).toContain("fileSystem");
				expect(maps.namespaceAliases.get("fs")).toBe("FS");
			});
		});

		describe("Edge Cases", () => {
			it("should handle empty files", () => {
				const code = "";
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				expect(maps.moduleAliases.size).toBe(0);
				expect(maps.namespaceAliases.size).toBe(0);
				expect(maps.namedImports.size).toBe(0);
			});

			it("should handle files with no imports", () => {
				const code = `
					const x = 5;
					function test() { return x; }
				`;
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				expect(maps.moduleAliases.size).toBe(0);
				expect(maps.namespaceAliases.size).toBe(0);
				expect(maps.namedImports.size).toBe(0);
			});

			it("should handle side-effect imports", () => {
				const code = `import 'polyfill';`;
				const sourceFile = createSourceFile(code);
				const maps = buildAliasMaps(sourceFile);

				// Side-effect imports have no bindings, shouldn't be tracked
				expect(maps.moduleAliases.size).toBe(0);
				expect(maps.namespaceAliases.size).toBe(0);
				expect(maps.namedImports.size).toBe(0);
			});
		});
	});

	describe("isAliasOf", () => {
		it("should detect module aliases (default imports)", () => {
			const code = `import fs from 'fs';`;
			const sourceFile = createSourceFile(code);
			const maps = buildAliasMaps(sourceFile);

			expect(isAliasOf("fs", "fs", maps)).toBe(true);
			expect(isAliasOf("path", "fs", maps)).toBe(false);
		});

		it("should detect namespace aliases", () => {
			const code = `import * as VM from 'vm';`;
			const sourceFile = createSourceFile(code);
			const maps = buildAliasMaps(sourceFile);

			expect(isAliasOf("VM", "vm", maps)).toBe(true);
			expect(isAliasOf("vm", "vm", maps)).toBe(false);
		});

		it("should detect named imports", () => {
			const code = `import { exec, spawn } from 'child_process';`;
			const sourceFile = createSourceFile(code);
			const maps = buildAliasMaps(sourceFile);

			expect(isAliasOf("exec", "child_process", maps)).toBe(true);
			expect(isAliasOf("spawn", "child_process", maps)).toBe(true);
		});

		it("should detect renamed named imports", () => {
			const code = `import { exec as execute } from 'child_process';`;
			const sourceFile = createSourceFile(code);
			const maps = buildAliasMaps(sourceFile);

			expect(isAliasOf("execute", "child_process", maps)).toBe(true);
			expect(isAliasOf("exec", "child_process", maps)).toBe(false);
		});

		it("should return false for non-aliased identifiers", () => {
			const code = `import fs from 'fs';`;
			const sourceFile = createSourceFile(code);
			const maps = buildAliasMaps(sourceFile);

			expect(isAliasOf("randomVar", "fs", maps)).toBe(false);
			expect(isAliasOf("cp", "child_process", maps)).toBe(false);
		});

		it("should return false for wrong module name", () => {
			const code = `import fs from 'fs';`;
			const sourceFile = createSourceFile(code);
			const maps = buildAliasMaps(sourceFile);

			expect(isAliasOf("fs", "child_process", maps)).toBe(false);
			expect(isAliasOf("fs", "path", maps)).toBe(false);
		});

		it("should handle multiple aliases correctly", () => {
			const code = `
				import fs from 'fs';
				const fileSystem = require('fs');
				import * as FS from 'fs';
				import { readFileSync as read } from 'fs';
			`;
			const sourceFile = createSourceFile(code);
			const maps = buildAliasMaps(sourceFile);

			expect(isAliasOf("fs", "fs", maps)).toBe(true);
			expect(isAliasOf("fileSystem", "fs", maps)).toBe(true);
			expect(isAliasOf("FS", "fs", maps)).toBe(true);
			expect(isAliasOf("read", "fs", maps)).toBe(true);
			expect(isAliasOf("other", "fs", maps)).toBe(false);
		});
	});
});
