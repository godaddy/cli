import { SEC102_CHILD_PROCESS } from "@/core/security/rules/bundle/SEC102-child-process.ts";
import { describe, expect, it } from "vitest";

describe("SEC102: child_process detection (bundled)", () => {
	it("has correct metadata", () => {
		expect(SEC102_CHILD_PROCESS.id).toBe("SEC102");
		expect(SEC102_CHILD_PROCESS.severity).toBe("block");
		expect(SEC102_CHILD_PROCESS.sourceRuleId).toBe("SEC002");
		expect(SEC102_CHILD_PROCESS.patterns.length).toBeGreaterThan(0);
		expect(SEC102_CHILD_PROCESS.signalPatterns).toBeDefined();
	});

	it("detects require child_process signal", () => {
		const code = 'const cp = require("child_process");';
		const hasMatch = SEC102_CHILD_PROCESS.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects node: prefix in require", () => {
		const code = 'const cp = require("node:child_process");';
		const hasMatch = SEC102_CHILD_PROCESS.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects ESM import signal", () => {
		const code = 'import { exec } from "child_process";';
		const hasMatch = SEC102_CHILD_PROCESS.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects bundler helper require_child_process", () => {
		const code = "const cp = require_child_process();";
		const hasMatch = SEC102_CHILD_PROCESS.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects esbuild __require helper", () => {
		const code = 'const cp = __require("child_process");';
		const hasMatch = SEC102_CHILD_PROCESS.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects exec() usage pattern", () => {
		const code = 'exec("ls -la");';
		const hasMatch = SEC102_CHILD_PROCESS.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects spawn() usage pattern", () => {
		const code = 'spawn("node", ["script.js"]);';
		const hasMatch = SEC102_CHILD_PROCESS.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects execSync() usage pattern", () => {
		const code = 'const output = execSync("pwd");';
		const hasMatch = SEC102_CHILD_PROCESS.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("does not match safe code without child_process", () => {
		const code = "function execute() { return 42; }";
		const hasSignal = SEC102_CHILD_PROCESS.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasSignal).toBe(false);
	});
});
