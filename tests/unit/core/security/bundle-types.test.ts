import { describe, expect, it } from "vitest";
import type { BundleRule, RuleId } from "../../../../src/core/security/types";

describe("Bundle Security Types", () => {
	it("BundleRule has all required fields", () => {
		const rule: BundleRule = {
			id: "SEC101",
			severity: "block",
			title: "Test Rule",
			description: "Test description",
			sourceRuleId: "SEC001",
			patterns: [/test/g],
		};

		expect(rule.id).toBe("SEC101");
		expect(rule.severity).toBe("block");
		expect(rule.patterns).toHaveLength(1);
		expect(rule.sourceRuleId).toBe("SEC001");
	});

	it("BundleRule accepts optional signalPatterns for two-pass detection", () => {
		const rule: BundleRule = {
			id: "SEC102",
			severity: "block",
			title: "Test Rule with Signals",
			description: "Test description",
			sourceRuleId: "SEC002",
			patterns: [/exec\s*\(/g],
			signalPatterns: [/require\s*\(\s*['"]child_process['"]\s*\)/g],
		};

		expect(rule.signalPatterns).toBeDefined();
		expect(rule.signalPatterns).toHaveLength(1);
	});

	it("accepts SEC101-SEC110 rule IDs", () => {
		const ids: RuleId[] = [
			"SEC101",
			"SEC102",
			"SEC103",
			"SEC104",
			"SEC105",
			"SEC106",
			"SEC107",
			"SEC108",
			"SEC109",
			"SEC110",
		];

		for (const id of ids) {
			expect(id).toMatch(/^SEC1\d{2}$/);
		}
	});
});
