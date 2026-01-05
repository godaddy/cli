import type * as ts from "typescript";

/**
 * Security rule identifiers
 */
export type RuleId =
	| "SEC001"
	| "SEC002"
	| "SEC003"
	| "SEC004"
	| "SEC005"
	| "SEC006"
	| "SEC007"
	| "SEC008"
	| "SEC009"
	| "SEC010"
	| "SEC011" // package.json scripts
	| "SEC101" // bundled rules
	| "SEC102"
	| "SEC103"
	| "SEC104"
	| "SEC105"
	| "SEC106"
	| "SEC107"
	| "SEC108"
	| "SEC109"
	| "SEC110";

/**
 * Severity level for security findings
 */
export type Severity = "off" | "warn" | "block";

/**
 * Metadata for a security rule
 */
export interface RuleMeta {
	id: RuleId;
	defaultSeverity: Severity;
	title: string;
	description: string;
	remediation: string;
	docsUrl?: string;
}

/**
 * A security finding detected during scanning
 */
export interface Finding {
	ruleId: RuleId;
	severity: Severity;
	message: string;
	file: string;
	line: number;
	col: number;
	snippet?: string;
}

/**
 * Module alias tracking for import/require statements
 */
export interface AliasMaps {
	/**
	 * Maps module name to set of local names (default imports, require assignments)
	 * e.g., "child_process" -> Set("cp", "childProcess")
	 */
	moduleAliases: Map<string, Set<string>>;

	/**
	 * Maps module name to namespace alias
	 * e.g., "vm" -> "VM" for `import * as VM from 'vm'`
	 */
	namespaceAliases: Map<string, string>;

	/**
	 * Maps module name to named imports
	 * e.g., "child_process" -> Map("exec" -> "execute", "spawn" -> "spawn")
	 */
	namedImports: Map<string, Map<string, string>>;
}

/**
 * Security configuration
 */
export interface SecurityConfig {
	mode: "strict";
	trustedDomains: string[];
	exclude: string[];
}

/**
 * Context passed to rule handlers
 */
export interface RuleContext {
	sourceFile: ts.SourceFile;
	filePath: string;
	config: SecurityConfig;
	aliasMaps: AliasMaps;
	report(message: string, node: ts.Node): void;
}

/**
 * Node visitor for AST traversal
 * Maps TypeScript SyntaxKind to handler functions
 */
export interface NodeVisitor {
	onFileStart?(): void;
	[kind: number]: ((node: ts.Node) => void) | undefined;
}

/**
 * Security rule definition
 */
export interface Rule {
	meta: RuleMeta;
	create(context: RuleContext): NodeVisitor;
}

/**
 * Regex-based security rule for bundle scanning.
 * Maps to source AST rules (SEC001-SEC010) with compiled patterns.
 */
export interface BundleRule {
	/** Rule identifier (SEC101-SEC110) */
	id: RuleId;

	/** Severity level determines deployment behavior */
	severity: Severity;

	/** Short rule title */
	title: string;

	/** Detailed description of threat */
	description: string;

	/** Original AST rule this maps to */
	sourceRuleId: RuleId;

	/**
	 * Compiled regex patterns to detect threats.
	 * Patterns are used with String.matchAll() for all occurrences.
	 * IMPORTANT: Use 'g' flag for global matching with matchAll().
	 */
	patterns: RegExp[];

	/**
	 * Optional: Import/require signal patterns for two-pass detection.
	 * If provided, matches from `patterns` only trigger when these signals are also found.
	 * Reduces false positives for method names like exec(), spawn() that may appear in safe code.
	 *
	 * Implementation: Scanner checks signals first; only flag pattern matches if signals found in same file.
	 */
	signalPatterns?: RegExp[];
}

/**
 * Summary statistics for scan report
 */
export interface ScanSummary {
	total: number;
	byRuleId: Record<string, number>;
	bySeverity: Record<Severity, number>;
}

/**
 * Complete scan report for an extension
 */
export interface ScanReport {
	findings: Finding[];
	blocked: boolean;
	summary: ScanSummary;
	scannedFiles: number;
}
