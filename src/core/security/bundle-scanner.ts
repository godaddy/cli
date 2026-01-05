import type { BundleRule, Finding } from "./types.ts";

/**
 * Calculate line number from character index in content.
 * Optimized for large bundles - no string splitting.
 *
 * **CRLF Normalization**: Treats `\r\n` as a single line break to avoid double-counting
 * on Windows-generated bundles.
 *
 * @param content - The full bundle content
 * @param charIndex - Zero-based character index in content
 * @returns One-based line number
 *
 * @example
 * ```ts
 * const content = "line1\nline2\neval()";
 * calculateLineNumber(content, 12); // Returns 3 (line with eval)
 *
 * // CRLF handling
 * const windowsContent = "line1\r\nline2";
 * calculateLineNumber(windowsContent, 7); // Returns 2, not 3
 * ```
 */
export function calculateLineNumber(
	content: string,
	charIndex: number,
): number {
	let lineNumber = 1;

	for (let i = 0; i < charIndex && i < content.length; i++) {
		if (content[i] === "\n") {
			lineNumber++;
		} else if (content[i] === "\r" && content[i + 1] === "\n") {
			// CRLF: count as one line break and skip the \n
			lineNumber++;
			i++; // Skip the \n in \r\n to avoid double-counting
		}
	}

	return lineNumber;
}

/**
 * Extract a code snippet around a match position.
 * Takes 25 characters before and after, trimmed at newline boundaries.
 *
 * @param content - The full bundle content
 * @param matchIndex - Index where the match was found
 * @returns Extracted snippet (max ~51 chars, trimmed at line boundaries)
 *
 * @example
 * ```ts
 * const content = "const x = eval('code');";
 * extractSnippet(content, 10); // Returns snippet around "eval"
 * ```
 */
export function extractSnippet(content: string, matchIndex: number): string {
	const contextChars = 25;
	const start = Math.max(0, matchIndex - contextChars);
	const end = Math.min(content.length, matchIndex + contextChars);

	let snippet = content.slice(start, end);
	let matchPositionInSnippet = matchIndex - start;

	// Trim at newline boundaries to avoid showing multiple lines
	const firstNewline = snippet.indexOf("\n");

	if (firstNewline !== -1 && firstNewline < matchPositionInSnippet) {
		// If there's a newline before our match, start after it
		snippet = snippet.slice(firstNewline + 1);
		matchPositionInSnippet -= firstNewline + 1;
	}

	// Now check for trailing newline after we've adjusted the snippet
	const lastNewline = snippet.lastIndexOf("\n");
	if (lastNewline !== -1 && lastNewline > matchPositionInSnippet) {
		// If there's a newline after our match, end before it
		snippet = snippet.slice(0, lastNewline);
	}

	return snippet.trim();
}

/**
 * Scan bundle content with regex-based security rules.
 * Implements two-pass detection for rules with signal patterns.
 *
 * **Two-Pass Detection:**
 * 1. If rule has `signalPatterns`, first check if any signals are present in content
 * 2. Only run main `patterns` if signals detected (reduces false positives)
 * 3. If no `signalPatterns`, run main patterns directly
 *
 * @param content - Bundle file content
 * @param rules - Array of BundleRule to apply
 * @param filePath - Path to bundle file (for reporting)
 * @returns Array of findings, sorted by line number
 *
 * @example
 * ```ts
 * const findings = scanBundleContent(bundleCode, BUNDLE_RULES, 'dist/bundle.mjs');
 * findings.forEach(f => console.log(`${f.ruleId} at line ${f.line}`));
 * ```
 */
export function scanBundleContent(
	content: string,
	rules: BundleRule[],
	filePath: string,
): Finding[] {
	const findings: Finding[] = [];

	for (const rule of rules) {
		// Determine which patterns to scan based on signal patterns
		let patternsToScan: RegExp[] = rule.patterns;

		if (rule.signalPatterns && rule.signalPatterns.length > 0) {
			// Check if any signal patterns match
			const hasSignal = rule.signalPatterns.some((signalPattern) => {
				// Create new RegExp to avoid stateful .test() with 'g' flag
				const testPattern = new RegExp(
					signalPattern.source,
					signalPattern.flags,
				);
				return testPattern.test(content);
			});

			if (!hasSignal) {
				// No signals found, skip this rule entirely
				continue;
			}

			// Signals found - scan both signal patterns AND main patterns
			patternsToScan = [...rule.signalPatterns, ...rule.patterns];
		}

		// Scan with all applicable patterns
		for (const pattern of patternsToScan) {
			// Use matchAll for global matching - requires 'g' flag
			const matches = content.matchAll(pattern);

			for (const match of matches) {
				if (match.index === undefined) continue;

				const line = calculateLineNumber(content, match.index);
				const snippet = extractSnippet(content, match.index);

				findings.push({
					ruleId: rule.id,
					severity: rule.severity,
					message: rule.description,
					file: filePath,
					line,
					col: 0, // Column not calculated for bundle scanning
					snippet,
				});
			}
		}
	}

	// Sort by line number
	findings.sort((a, b) => a.line - b.line);

	return findings;
}
