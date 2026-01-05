import { describe, expect, it } from "vitest";
import {
	calculateLineNumber,
	extractSnippet,
	scanBundleContent,
} from "../../../../src/core/security/bundle-scanner";
import { BUNDLE_RULES } from "../../../../src/core/security/rules/bundle";

describe("calculateLineNumber", () => {
	it("returns 1 for index 0", () => {
		const content = "line1\nline2\nline3";
		expect(calculateLineNumber(content, 0)).toBe(1);
	});

	it("calculates line number for single newline", () => {
		const content = "line1\nline2\nline3";
		// Index 6 = 'l' in "line2" (after "line1\n")
		expect(calculateLineNumber(content, 6)).toBe(2);
	});

	it("calculates line number for multiple newlines", () => {
		const content = "line1\nline2\nline3\nline4";
		// Index 12 = 'l' in "line3" (after "line1\nline2\n")
		expect(calculateLineNumber(content, 12)).toBe(3);
		// Index 18 = 'l' in "line4" (after "line1\nline2\nline3\n")
		expect(calculateLineNumber(content, 18)).toBe(4);
	});

	it("normalizes CRLF to avoid double-counting", () => {
		// Windows line endings: \r\n should count as one line break
		const content = "line1\r\nline2\r\nline3";
		// Index 7 = 'l' in "line2" (after "line1\r\n")
		expect(calculateLineNumber(content, 7)).toBe(2);
		// Index 14 = 'l' in "line3" (after "line1\r\nline2\r\n")
		expect(calculateLineNumber(content, 14)).toBe(3);
	});

	it("handles mixed line endings", () => {
		const content = "line1\nline2\r\nline3\nline4";
		// Index 6 = 'l' in "line2" (after "line1\n")
		expect(calculateLineNumber(content, 6)).toBe(2);
		// Index 13 = 'l' in "line3" (after "line1\nline2\r\n")
		expect(calculateLineNumber(content, 13)).toBe(3);
		// Index 19 = 'l' in "line4" (after "line1\nline2\r\nline3\n")
		expect(calculateLineNumber(content, 19)).toBe(4);
	});

	it("handles index exactly on newline character", () => {
		const content = "line1\nline2\nline3";
		// Index 5 = '\n' after "line1"
		expect(calculateLineNumber(content, 5)).toBe(1);
	});

	it("handles index at end of content", () => {
		const content = "line1\nline2\nline3";
		const lastIndex = content.length - 1;
		expect(calculateLineNumber(content, lastIndex)).toBe(3);
	});

	it("handles content with no newlines", () => {
		const content = "single line content";
		expect(calculateLineNumber(content, 0)).toBe(1);
		expect(calculateLineNumber(content, 5)).toBe(1);
		expect(calculateLineNumber(content, content.length - 1)).toBe(1);
	});

	it("handles empty content", () => {
		const content = "";
		expect(calculateLineNumber(content, 0)).toBe(1);
	});

	it("handles content with only newlines", () => {
		const content = "\n\n\n";
		expect(calculateLineNumber(content, 0)).toBe(1);
		expect(calculateLineNumber(content, 1)).toBe(2);
		expect(calculateLineNumber(content, 2)).toBe(3);
	});
});

describe("extractSnippet", () => {
	it("extracts snippet from middle of line", () => {
		const content = "This is a test line with eval() call";
		const match = content.match(/eval\(\)/)!;
		const snippet = extractSnippet(content, match.index!);
		expect(snippet).toContain("eval()");
		expect(snippet.length).toBeLessThanOrEqual(51); // 25 before + 25 after + match
	});

	it("handles snippet at start of content", () => {
		const content = "eval() at the beginning";
		const snippet = extractSnippet(content, 0);
		expect(snippet).toContain("eval()");
	});

	it("handles snippet at end of content", () => {
		const content = "ending with eval()";
		const match = content.match(/eval\(\)/)!;
		const snippet = extractSnippet(content, match.index!);
		expect(snippet).toContain("eval()");
	});

	it("truncates at newline boundaries", () => {
		const content = "line1\nline2 with eval() here\nline3";
		const match = content.match(/eval\(\)/)!;
		const snippet = extractSnippet(content, match.index!);
		expect(snippet).not.toContain("line1");
		expect(snippet).not.toContain("line3");
		expect(snippet).toContain("eval()");
	});
});

describe("scanBundleContent", () => {
	it("returns empty array for clean code", () => {
		const code = 'export function greet() { return "hello"; }';
		const findings = scanBundleContent(code, BUNDLE_RULES, "test.mjs");
		expect(findings).toEqual([]);
	});

	it("detects eval usage", () => {
		const malicious = 'function bad() { eval("malicious"); }';
		const findings = scanBundleContent(malicious, BUNDLE_RULES, "test.mjs");
		expect(findings.length).toBeGreaterThan(0);
		const evalFinding = findings.find((f) => f.ruleId === "SEC101");
		expect(evalFinding).toBeDefined();
		expect(evalFinding?.severity).toBe("block");
	});

	it("detects child_process require", () => {
		const malicious = 'var cp = require("child_process");';
		const findings = scanBundleContent(malicious, BUNDLE_RULES, "test.mjs");
		expect(findings.some((f) => f.ruleId === "SEC102")).toBe(true);
	});

	it("includes line numbers in findings", () => {
		const code = 'line1\nline2\neval("bad")\nline4';
		const findings = scanBundleContent(code, BUNDLE_RULES, "test.mjs");
		expect(findings[0].line).toBeGreaterThan(0);
	});

	it("includes snippets in findings", () => {
		const code = 'eval("malicious")';
		const findings = scanBundleContent(code, BUNDLE_RULES, "test.mjs");
		expect(findings[0].snippet).toBeTruthy();
	});

	it("sorts findings by line number", () => {
		const code = 'line1\neval("a")\nline3\neval("b")';
		const findings = scanBundleContent(code, BUNDLE_RULES, "test.mjs");
		if (findings.length > 1) {
			expect(findings[0].line).toBeLessThanOrEqual(findings[1].line);
		}
	});

	it("detects multiple violations in same bundle", () => {
		const code = `
      require("child_process");
      eval("code");
      require("vm");
    `;
		const findings = scanBundleContent(code, BUNDLE_RULES, "test.mjs");
		expect(findings.length).toBeGreaterThan(1);
	});
});
