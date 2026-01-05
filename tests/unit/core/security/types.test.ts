import type {
	AliasMaps,
	Finding,
	NodeVisitor,
	Rule,
	RuleContext,
	RuleId,
	RuleMeta,
	ScanReport,
	ScanSummary,
	SecurityConfig,
	Severity,
} from "@/core/security/types.ts";
import type * as ts from "typescript";
import { describe, expect, it } from "vitest";

describe("Security Types", () => {
	describe("RuleId", () => {
		it("should accept valid rule identifiers", () => {
			const validIds: RuleId[] = [
				"SEC001",
				"SEC002",
				"SEC003",
				"SEC004",
				"SEC005",
				"SEC006",
				"SEC007",
				"SEC008",
				"SEC009",
				"SEC010",
				"SEC011",
			];

			expect(validIds).toHaveLength(11);
			expect(validIds[0]).toBe("SEC001");
			expect(validIds[10]).toBe("SEC011");
		});
	});

	describe("Severity", () => {
		it("should accept valid severity levels", () => {
			const severities: Severity[] = ["off", "warn", "block"];

			expect(severities).toHaveLength(3);
			expect(severities).toContain("off");
			expect(severities).toContain("warn");
			expect(severities).toContain("block");
		});
	});

	describe("RuleMeta", () => {
		it("should have correct structure", () => {
			const meta: RuleMeta = {
				id: "SEC001",
				defaultSeverity: "block",
				title: "Test Rule",
				description: "Test description",
				remediation: "Test remediation",
			};

			expect(meta.id).toBe("SEC001");
			expect(meta.defaultSeverity).toBe("block");
			expect(meta.title).toBe("Test Rule");
			expect(meta.description).toBe("Test description");
			expect(meta.remediation).toBe("Test remediation");
		});

		it("should allow optional docsUrl", () => {
			const metaWithDocs: RuleMeta = {
				id: "SEC002",
				defaultSeverity: "warn",
				title: "Test Rule",
				description: "Test description",
				remediation: "Test remediation",
				docsUrl: "https://docs.example.com",
			};

			expect(metaWithDocs.docsUrl).toBe("https://docs.example.com");
		});
	});

	describe("Finding", () => {
		it("should have correct structure", () => {
			const finding: Finding = {
				ruleId: "SEC001",
				severity: "block",
				message: "eval() detected",
				file: "/path/to/file.ts",
				line: 10,
				col: 5,
			};

			expect(finding.ruleId).toBe("SEC001");
			expect(finding.severity).toBe("block");
			expect(finding.message).toBe("eval() detected");
			expect(finding.file).toBe("/path/to/file.ts");
			expect(finding.line).toBe(10);
			expect(finding.col).toBe(5);
		});

		it("should allow optional snippet", () => {
			const finding: Finding = {
				ruleId: "SEC001",
				severity: "block",
				message: "eval() detected",
				file: "/path/to/file.ts",
				line: 10,
				col: 5,
				snippet: "eval(userInput)",
			};

			expect(finding.snippet).toBe("eval(userInput)");
		});
	});

	describe("AliasMaps", () => {
		it("should have correct structure", () => {
			const maps: AliasMaps = {
				moduleAliases: new Map([["child_process", new Set(["cp"])]]),
				namespaceAliases: new Map([["vm", "VM"]]),
				namedImports: new Map([
					["child_process", new Map([["exec", "execute"]])],
				]),
			};

			expect(maps.moduleAliases.get("child_process")).toContain("cp");
			expect(maps.namespaceAliases.get("vm")).toBe("VM");
			expect(maps.namedImports.get("child_process")?.get("exec")).toBe(
				"execute",
			);
		});

		it("should support multiple aliases per module", () => {
			const maps: AliasMaps = {
				moduleAliases: new Map([
					["child_process", new Set(["cp", "childProcess"])],
				]),
				namespaceAliases: new Map(),
				namedImports: new Map([
					[
						"child_process",
						new Map([
							["exec", "execute"],
							["spawn", "spawn"],
						]),
					],
				]),
			};

			expect(maps.moduleAliases.get("child_process")?.size).toBe(2);
			expect(maps.namedImports.get("child_process")?.size).toBe(2);
		});
	});

	describe("SecurityConfig", () => {
		it("should have strict mode and required fields", () => {
			const config: SecurityConfig = {
				mode: "strict",
				trustedDomains: ["*.godaddy.com", "localhost"],
				exclude: ["**/node_modules/**"],
			};

			expect(config.mode).toBe("strict");
			expect(config.trustedDomains).toContain("*.godaddy.com");
			expect(config.exclude).toContain("**/node_modules/**");
		});
	});

	describe("RuleContext", () => {
		it("should have correct structure", () => {
			const mockSourceFile = {} as ts.SourceFile;
			const mockAliasMaps: AliasMaps = {
				moduleAliases: new Map(),
				namespaceAliases: new Map(),
				namedImports: new Map(),
			};
			const mockConfig: SecurityConfig = {
				mode: "strict",
				trustedDomains: [],
				exclude: [],
			};

			const context: RuleContext = {
				sourceFile: mockSourceFile,
				filePath: "/test/file.ts",
				config: mockConfig,
				aliasMaps: mockAliasMaps,
				report: () => {},
			};

			expect(context.filePath).toBe("/test/file.ts");
			expect(context.config.mode).toBe("strict");
			expect(typeof context.report).toBe("function");
		});
	});

	describe("NodeVisitor", () => {
		it("should allow numeric keys for SyntaxKind handlers", () => {
			const visitor: NodeVisitor = {
				onFileStart: () => {},
				[123]: (node: ts.Node) => {},
			};

			expect(typeof visitor.onFileStart).toBe("function");
			expect(typeof visitor[123]).toBe("function");
		});

		it("should allow visitor without onFileStart", () => {
			const visitor: NodeVisitor = {
				[200]: (node: ts.Node) => {},
			};

			expect(visitor.onFileStart).toBeUndefined();
			expect(typeof visitor[200]).toBe("function");
		});
	});

	describe("Rule", () => {
		it("should have correct structure", () => {
			const rule: Rule = {
				meta: {
					id: "SEC001",
					defaultSeverity: "block",
					title: "Test",
					description: "Test",
					remediation: "Test",
				},
				create: (context: RuleContext) => ({
					onFileStart: () => {},
				}),
			};

			expect(rule.meta.id).toBe("SEC001");
			expect(typeof rule.create).toBe("function");
		});
	});

	describe("ScanSummary", () => {
		it("should have correct structure", () => {
			const summary: ScanSummary = {
				total: 5,
				byRuleId: {
					SEC001: 2,
					SEC002: 3,
				},
				bySeverity: {
					off: 0,
					warn: 2,
					block: 3,
				},
			};

			expect(summary.total).toBe(5);
			expect(summary.byRuleId.SEC001).toBe(2);
			expect(summary.bySeverity.block).toBe(3);
		});
	});

	describe("ScanReport", () => {
		it("should have correct structure", () => {
			const report: ScanReport = {
				findings: [],
				blocked: false,
				summary: {
					total: 0,
					byRuleId: {},
					bySeverity: {
						off: 0,
						warn: 0,
						block: 0,
					},
				},
				scannedFiles: 10,
			};

			expect(report.findings).toEqual([]);
			expect(report.blocked).toBe(false);
			expect(report.summary.total).toBe(0);
			expect(report.scannedFiles).toBe(10);
		});

		it("should mark as blocked when block-severity findings exist", () => {
			const report: ScanReport = {
				findings: [
					{
						ruleId: "SEC001",
						severity: "block",
						message: "eval() detected",
						file: "/test.ts",
						line: 1,
						col: 1,
					},
				],
				blocked: true,
				summary: {
					total: 1,
					byRuleId: { SEC001: 1 },
					bySeverity: {
						off: 0,
						warn: 0,
						block: 1,
					},
				},
				scannedFiles: 1,
			};

			expect(report.blocked).toBe(true);
			expect(report.findings).toHaveLength(1);
			expect(report.summary.bySeverity.block).toBe(1);
		});
	});
});
