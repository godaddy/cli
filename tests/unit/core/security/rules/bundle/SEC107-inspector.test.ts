import { SEC107_INSPECTOR } from "@/core/security/rules/bundle/SEC107-inspector.ts";
import { describe, expect, it } from "vitest";

describe("SEC107: inspector module detection (bundled)", () => {
	it("has correct metadata", () => {
		expect(SEC107_INSPECTOR.id).toBe("SEC107");
		expect(SEC107_INSPECTOR.severity).toBe("block");
		expect(SEC107_INSPECTOR.sourceRuleId).toBe("SEC007");
		expect(SEC107_INSPECTOR.patterns.length).toBeGreaterThan(0);
		expect(SEC107_INSPECTOR.signalPatterns).toBeDefined();
	});

	it("detects require inspector signal", () => {
		const code = 'const inspector = require("inspector");';
		const hasMatch = SEC107_INSPECTOR.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects node: prefix in require", () => {
		const code = 'const inspector = require("node:inspector");';
		const hasMatch = SEC107_INSPECTOR.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects ESM import signal", () => {
		const code = 'import inspector from "inspector";';
		const hasMatch = SEC107_INSPECTOR.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects bundler helper require_inspector", () => {
		const code = "const inspector = require_inspector();";
		const hasMatch = SEC107_INSPECTOR.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects inspector.open usage", () => {
		const code = "inspector.open(9229);";
		const hasMatch = SEC107_INSPECTOR.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects inspector.url usage", () => {
		const code = "const url = inspector.url();";
		const hasMatch = SEC107_INSPECTOR.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects inspector.waitForDebugger usage", () => {
		const code = "inspector.waitForDebugger();";
		const hasMatch = SEC107_INSPECTOR.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects bracket notation inspector['open']", () => {
		const code = "inspector['open'](9229);";
		const hasMatch = SEC107_INSPECTOR.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("does not match safe code", () => {
		const code = "const inspect = require('util').inspect;";
		const hasSignal = SEC107_INSPECTOR.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasSignal).toBe(false);
	});
});
