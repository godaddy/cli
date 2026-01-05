import { SEC108_EXTERNAL_URL } from "@/core/security/rules/bundle/SEC108-external-url.ts";
import { describe, expect, it } from "vitest";

describe("SEC108: external URL detection (bundled)", () => {
	it("has correct metadata", () => {
		expect(SEC108_EXTERNAL_URL.id).toBe("SEC108");
		expect(SEC108_EXTERNAL_URL.severity).toBe("warn");
		expect(SEC108_EXTERNAL_URL.sourceRuleId).toBe("SEC008");
		expect(SEC108_EXTERNAL_URL.patterns.length).toBeGreaterThan(0);
		expect(SEC108_EXTERNAL_URL.signalPatterns).toBeDefined();
	});

	it("detects require http signal", () => {
		const code = 'const http = require("http");';
		const hasMatch = SEC108_EXTERNAL_URL.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects require https signal", () => {
		const code = 'const https = require("https");';
		const hasMatch = SEC108_EXTERNAL_URL.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects require axios signal", () => {
		const code = 'const axios = require("axios");';
		const hasMatch = SEC108_EXTERNAL_URL.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects HTTP URL pattern", () => {
		const code = 'const url = "http://example.com/api";';
		const hasMatch = SEC108_EXTERNAL_URL.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects HTTPS URL pattern", () => {
		const code = 'const url = "https://api.example.com/data";';
		const hasMatch = SEC108_EXTERNAL_URL.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects new URL() constructor", () => {
		const code = 'new URL("https://example.com")';
		const hasMatch = SEC108_EXTERNAL_URL.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects fetch() with URL", () => {
		const code = 'fetch("https://api.example.com/endpoint")';
		const hasMatch = SEC108_EXTERNAL_URL.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("does not match relative URLs", () => {
		const code = 'fetch("/api/users")';
		const hasMatch = SEC108_EXTERNAL_URL.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(false);
	});
});
