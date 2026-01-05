import { SEC104_PROCESS_BINDING } from "@/core/security/rules/bundle/SEC104-process-binding.ts";
import { describe, expect, it } from "vitest";

describe("SEC104: process.binding detection (bundled)", () => {
	it("has correct metadata", () => {
		expect(SEC104_PROCESS_BINDING.id).toBe("SEC104");
		expect(SEC104_PROCESS_BINDING.severity).toBe("block");
		expect(SEC104_PROCESS_BINDING.sourceRuleId).toBe("SEC004");
		expect(SEC104_PROCESS_BINDING.patterns.length).toBeGreaterThan(0);
	});

	it("detects process.binding(", () => {
		const code = 'const natives = process.binding("natives");';
		const hasMatch = SEC104_PROCESS_BINDING.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects process._linkedBinding(", () => {
		const code = 'const binding = process._linkedBinding("fs");';
		const hasMatch = SEC104_PROCESS_BINDING.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects process.dlopen(", () => {
		const code = 'process.dlopen(module, "addon.node");';
		const hasMatch = SEC104_PROCESS_BINDING.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects internalBinding(", () => {
		const code = 'const internal = internalBinding("util");';
		const hasMatch = SEC104_PROCESS_BINDING.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects bracket notation process['binding']", () => {
		const code = 'const b = process["binding"]("fs");';
		const hasMatch = SEC104_PROCESS_BINDING.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects bracket notation process['dlopen']", () => {
		const code = "process['dlopen'](mod, path);";
		const hasMatch = SEC104_PROCESS_BINDING.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("does not match safe process usage", () => {
		const code = "console.log(process.env.NODE_ENV);";
		const hasMatch = SEC104_PROCESS_BINDING.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(false);
	});
});
