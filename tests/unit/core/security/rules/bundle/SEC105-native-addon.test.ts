import { SEC105_NATIVE_ADDON } from "@/core/security/rules/bundle/SEC105-native-addon.ts";
import { describe, expect, it } from "vitest";

describe("SEC105: native addon detection (bundled)", () => {
	it("has correct metadata", () => {
		expect(SEC105_NATIVE_ADDON.id).toBe("SEC105");
		expect(SEC105_NATIVE_ADDON.severity).toBe("block");
		expect(SEC105_NATIVE_ADDON.sourceRuleId).toBe("SEC005");
		expect(SEC105_NATIVE_ADDON.patterns.length).toBeGreaterThan(0);
		expect(SEC105_NATIVE_ADDON.signalPatterns).toBeDefined();
	});

	it("detects require bindings signal", () => {
		const code = 'const bindings = require("bindings");';
		const hasMatch = SEC105_NATIVE_ADDON.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects require ffi-napi signal", () => {
		const code = 'const ffi = require("ffi-napi");';
		const hasMatch = SEC105_NATIVE_ADDON.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects require node-addon-api signal", () => {
		const code = 'const addon = require("node-addon-api");';
		const hasMatch = SEC105_NATIVE_ADDON.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects .node file extension pattern", () => {
		const code = 'require("./build/addon.node")';
		const hasMatch = SEC105_NATIVE_ADDON.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects .node in double quotes", () => {
		const code = 'const addon = require("native.node");';
		const hasMatch = SEC105_NATIVE_ADDON.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects process.dlopen usage", () => {
		const code = 'process.dlopen(module, "./addon.node");';
		const hasMatch = SEC105_NATIVE_ADDON.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("does not match safe code", () => {
		const code = 'const config = require("./config.json");';
		const hasSignal = SEC105_NATIVE_ADDON.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasSignal).toBe(false);
	});
});
