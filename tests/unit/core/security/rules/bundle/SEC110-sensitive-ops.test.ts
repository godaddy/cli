import { SEC110_SENSITIVE_OPS } from "@/core/security/rules/bundle/SEC110-sensitive-ops.ts";
import { describe, expect, it } from "vitest";

describe("SEC110: sensitive operations detection (bundled)", () => {
	it("has correct metadata", () => {
		expect(SEC110_SENSITIVE_OPS.id).toBe("SEC110");
		expect(SEC110_SENSITIVE_OPS.severity).toBe("warn");
		expect(SEC110_SENSITIVE_OPS.sourceRuleId).toBe("SEC010");
		expect(SEC110_SENSITIVE_OPS.patterns.length).toBeGreaterThan(0);
		expect(SEC110_SENSITIVE_OPS.signalPatterns).toBeDefined();
	});

	it("detects require net signal", () => {
		const code = 'const net = require("net");';
		const hasMatch = SEC110_SENSITIVE_OPS.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects require fs signal", () => {
		const code = 'const fs = require("fs");';
		const hasMatch = SEC110_SENSITIVE_OPS.signalPatterns!.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects net.connect usage", () => {
		const code = "net.connect(8080, 'localhost');";
		const hasMatch = SEC110_SENSITIVE_OPS.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects net.createConnection usage", () => {
		const code = "net.createConnection({port: 22});";
		const hasMatch = SEC110_SENSITIVE_OPS.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects process.env[] access", () => {
		const code = 'const key = process.env["API_KEY"];';
		const hasMatch = SEC110_SENSITIVE_OPS.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects process.env. access", () => {
		const code = "const secret = process.env.SECRET;";
		const hasMatch = SEC110_SENSITIVE_OPS.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects /etc/passwd path", () => {
		const code = 'fs.readFileSync("/etc/passwd");';
		const hasMatch = SEC110_SENSITIVE_OPS.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects /etc/shadow path", () => {
		const code = 'const shadow = "/etc/shadow";';
		const hasMatch = SEC110_SENSITIVE_OPS.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects .ssh/ path", () => {
		const code = 'const keyPath = "/.ssh/id_rsa";';
		const hasMatch = SEC110_SENSITIVE_OPS.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects ~/.ssh/ path", () => {
		const code = 'const sshKey = "~/.ssh/id_rsa";';
		const hasMatch = SEC110_SENSITIVE_OPS.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("detects .env file reads", () => {
		const code = 'fs.readFileSync(".env");';
		const hasMatch = SEC110_SENSITIVE_OPS.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(true);
	});

	it("does not match safe process usage", () => {
		const code = "console.log(process.version);";
		const hasMatch = SEC110_SENSITIVE_OPS.patterns.some((p) =>
			new RegExp(p.source).test(code),
		);
		expect(hasMatch).toBe(false);
	});
});
