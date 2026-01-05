import { readPackageJson } from "../../services/extension/workspace";
import type { Result } from "../../shared/types";
import type { Finding } from "./types";

/**
 * Suspicious command patterns detected in package.json scripts
 */
interface SuspiciousPattern {
	/** Pattern name for error messages */
	name: string;
	/** Regex to match against script content */
	pattern: RegExp;
	/** Explanation of why this pattern is suspicious */
	reason: string;
}

/**
 * package.json structure with scripts field
 */
interface PackageJson {
	name?: string;
	version?: string;
	scripts?: Record<string, string>;
	[key: string]: unknown;
}

/**
 * Lifecycle scripts that execute automatically during installation
 */
const LIFECYCLE_SCRIPTS = ["install", "postinstall", "preinstall"] as const;

/**
 * Scan package.json scripts for suspicious commands that could indicate malicious behavior.
 * Focuses on lifecycle scripts (install, postinstall, preinstall) that execute automatically.
 *
 * @param pkgPath - Absolute path to package.json file
 * @returns Result containing array of security findings (SEC011) or error
 *
 * @example
 * ```ts
 * const result = await scanPackageScripts('/path/to/package.json');
 * if (result.success && result.data) {
 *   for (const finding of result.data) {
 *     console.log(`${finding.ruleId}: ${finding.message}`);
 *   }
 * }
 * ```
 */
export function scanPackageScripts(pkgPath: string): Result<Finding[]> {
	// Read package.json
	const pkgResult = readPackageJson(pkgPath);
	if (!pkgResult.success) {
		return {
			success: false,
			error: pkgResult.error || new Error("Failed to read package.json"),
		};
	}

	if (!pkgResult.data) {
		return {
			success: false,
			error: new Error("Failed to read package.json"),
		};
	}

	const pkg = pkgResult.data;
	const findings: Finding[] = [];

	// No scripts to scan
	if (!pkg.scripts || typeof pkg.scripts !== "object") {
		return { success: true, data: findings };
	}

	// Get suspicious patterns
	const patterns = getSuspiciousPatterns();

	// Scan only lifecycle scripts
	const scripts = pkg.scripts as Record<string, string>;
	for (const scriptName of LIFECYCLE_SCRIPTS) {
		const scriptContent = scripts[scriptName];
		if (scriptContent && typeof scriptContent === "string") {
			const finding = checkScriptForPatterns(
				scriptName,
				scriptContent,
				patterns,
				pkgPath,
			);
			if (finding) {
				findings.push(finding);
			}
		}
	}

	return { success: true, data: findings };
}

/**
 * Get list of suspicious command patterns to detect in scripts.
 * Includes download tools, shell execution, encoded commands, and network/IPC primitives.
 *
 * @returns Array of suspicious pattern definitions
 */
function getSuspiciousPatterns(): SuspiciousPattern[] {
	return [
		{
			name: "curl",
			pattern: /\bcurl\b/i,
			reason: "Download tool that can fetch remote payloads",
		},
		{
			name: "wget",
			pattern: /\bwget\b/i,
			reason: "Download tool that can fetch remote payloads",
		},
		{
			name: "bash -c",
			pattern: /\bbash\s+-c\b/i,
			reason: "Arbitrary command execution via bash",
		},
		{
			name: "sh -c",
			pattern: /\bsh\s+-c\b/i,
			reason: "Arbitrary command execution via shell",
		},
		{
			name: "powershell -enc",
			pattern: /\bpowershell\s+-enc\b/i,
			reason: "Encoded PowerShell command execution",
		},
		{
			name: "nc",
			pattern: /\bnc\b/i,
			reason: "Network utility that can create backdoors",
		},
		{
			name: "mkfifo",
			pattern: /\bmkfifo\b/i,
			reason: "Create named pipes for inter-process communication",
		},
		{
			name: "eval",
			pattern: /\beval\b/i,
			reason: "Dynamic code evaluation in shell context",
		},
		{
			name: "exec",
			pattern: /\bexec\b/i,
			reason: "Command execution in shell context",
		},
	];
}

/**
 * Check if a script contains any suspicious patterns.
 *
 * @param scriptName - Name of the script (e.g., "postinstall")
 * @param scriptContent - Content of the script command
 * @param patterns - Array of suspicious patterns to check against
 * @param pkgPath - Path to package.json for finding location
 * @returns Finding if pattern detected, undefined otherwise
 */
function checkScriptForPatterns(
	scriptName: string,
	scriptContent: string,
	patterns: SuspiciousPattern[],
	pkgPath: string,
): Finding | undefined {
	for (const pattern of patterns) {
		if (pattern.pattern.test(scriptContent)) {
			return createScriptFinding(scriptName, pattern, pkgPath);
		}
	}
	return undefined;
}

/**
 * Create a security finding for a suspicious script.
 *
 * @param scriptName - Name of the script (e.g., "postinstall")
 * @param pattern - Matched suspicious pattern
 * @param pkgPath - Path to package.json for finding location
 * @returns Finding object with SEC011 rule ID
 */
function createScriptFinding(
	scriptName: string,
	pattern: SuspiciousPattern,
	pkgPath: string,
): Finding {
	return {
		ruleId: "SEC011",
		severity: "warn",
		message: `Suspicious command '${pattern.name}' in ${scriptName} script: ${pattern.reason}`,
		file: pkgPath,
		line: 0,
		col: 0,
	};
}
