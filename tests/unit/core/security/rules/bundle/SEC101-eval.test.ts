import { SEC101_EVAL } from "@/core/security/rules/bundle/SEC101-eval.ts";
import { describe, expect, it } from "vitest";

describe("SEC101: eval detection (bundled)", () => {
	it("has correct metadata", () => {
		expect(SEC101_EVAL.id).toBe("SEC101");
		expect(SEC101_EVAL.severity).toBe("block");
		expect(SEC101_EVAL.sourceRuleId).toBe("SEC001");
		expect(SEC101_EVAL.patterns.length).toBeGreaterThan(0);
	});

	it("detects eval(", () => {
		const code = 'function bad() { eval("malicious"); }';
		// NOTE: Create fresh RegExp to avoid stateful test() issues with 'g' flag
		const hasMatch = SEC101_EVAL.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects new Function(", () => {
		const code = 'const fn = new Function("return 1");';
		const hasMatch = SEC101_EVAL.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects obfuscated eval - bracket notation", () => {
		const code = `globalThis['eval']("code");`;
		const hasMatch = SEC101_EVAL.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("does not match safe code", () => {
		const code = 'const evaluation = "test"; // eval in comment';
		const hasMatch = SEC101_EVAL.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(false);
	});

	it("detects globalThis.eval(", () => {
		const code = "globalThis.eval('code')";
		const hasMatch = SEC101_EVAL.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects window.eval(", () => {
		const code = "window.eval('code')";
		const hasMatch = SEC101_EVAL.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects obfuscated decode→sink pattern", () => {
		const code = 'eval(atob("base64code"))';
		const hasMatch = SEC101_EVAL.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects hex-escaped eval", () => {
		const code = 'eval("\\x61\\x6c\\x65\\x72\\x74\\x28\\x31\\x29")';
		const hasMatch = SEC101_EVAL.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});
});
