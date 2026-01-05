import { SEC103_VM } from "@/core/security/rules/bundle/SEC103-vm.ts";
import { describe, expect, it } from "vitest";

describe("SEC103: vm module detection (bundled)", () => {
	it("has correct metadata", () => {
		expect(SEC103_VM.id).toBe("SEC103");
		expect(SEC103_VM.severity).toBe("block");
		expect(SEC103_VM.sourceRuleId).toBe("SEC003");
		expect(SEC103_VM.patterns.length).toBeGreaterThan(0);
		expect(SEC103_VM.signalPatterns).toBeDefined();
	});

	it("detects require vm signal", () => {
		const code = 'const vm = require("vm");';
		const hasMatch = SEC103_VM.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects node: prefix in require", () => {
		const code = 'const vm = require("node:vm");';
		const hasMatch = SEC103_VM.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects ESM import signal", () => {
		const code = 'import vm from "vm";';
		const hasMatch = SEC103_VM.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects bundler helper require_vm", () => {
		const code = "const vm = require_vm();";
		const hasMatch = SEC103_VM.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects Script constructor usage", () => {
		const code = 'new Script("code");';
		const hasMatch = SEC103_VM.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects runInNewContext usage", () => {
		const code = "script.runInNewContext({});";
		const hasMatch = SEC103_VM.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects runInContext usage", () => {
		const code = "script.runInContext(context);";
		const hasMatch = SEC103_VM.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects createContext usage", () => {
		const code = "const ctx = vm.createContext({});";
		const hasMatch = SEC103_VM.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("does not match safe code without vm module", () => {
		const code = "const script = 'hello world';";
		const hasSignal = SEC103_VM.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasSignal).toBe(false);
	});
});
