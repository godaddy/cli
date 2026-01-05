import { SEC106_MODULE_PATCH } from "@/core/security/rules/bundle/SEC106-module-patch.ts";
import { describe, expect, it } from "vitest";

describe("SEC106: module monkey-patching detection (bundled)", () => {
	it("has correct metadata", () => {
		expect(SEC106_MODULE_PATCH.id).toBe("SEC106");
		expect(SEC106_MODULE_PATCH.severity).toBe("block");
		expect(SEC106_MODULE_PATCH.sourceRuleId).toBe("SEC006");
		expect(SEC106_MODULE_PATCH.patterns.length).toBeGreaterThan(0);
		expect(SEC106_MODULE_PATCH.signalPatterns).toBeDefined();
	});

	it("detects require module signal", () => {
		const code = 'const Module = require("module");';
		const hasMatch = SEC106_MODULE_PATCH.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects ESM import module signal", () => {
		const code = 'import Module from "module";';
		const hasMatch = SEC106_MODULE_PATCH.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects Module._load assignment", () => {
		const code = "Module._load = function() { };";
		const hasMatch = SEC106_MODULE_PATCH.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects Module._resolveFilename assignment", () => {
		const code = "Module._resolveFilename = customResolver;";
		const hasMatch = SEC106_MODULE_PATCH.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects Module._extensions modification", () => {
		const code = 'Module._extensions[".custom"] = loader;';
		const hasMatch = SEC106_MODULE_PATCH.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects require.cache modification", () => {
		const code = 'require.cache["/path/to/module"] = {};';
		const hasMatch = SEC106_MODULE_PATCH.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects require.cache deletion", () => {
		const code = 'delete require.cache[require.resolve("./module")];';
		const hasMatch = SEC106_MODULE_PATCH.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects bracket notation Module['_load']", () => {
		const code = "Module['_load'] = hijacked;";
		const hasMatch = SEC106_MODULE_PATCH.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("does not match safe code", () => {
		const code = "const exports = module.exports;";
		const hasMatch = SEC106_MODULE_PATCH.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(false);
	});
});
