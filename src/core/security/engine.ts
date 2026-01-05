import ts from "typescript";
import type {
	AliasMaps,
	Finding,
	NodeVisitor,
	Rule,
	RuleContext,
	SecurityConfig,
} from "./types.ts";

export function scanFile(
	filePath: string,
	sourceText: string,
	rules: Rule[],
	config: SecurityConfig,
	aliasMaps: AliasMaps,
): Finding[] {
	const sourceFile = ts.createSourceFile(
		filePath,
		sourceText,
		ts.ScriptTarget.Latest,
		true,
		ts.ScriptKind.TSX,
	);

	const findings: Finding[] = [];
	const visitors: NodeVisitor[] = [];

	// Create visitors for each rule
	for (const rule of rules) {
		const ctx: RuleContext = {
			sourceFile,
			filePath,
			config,
			aliasMaps,
			report: (message: string, node: ts.Node) => {
				findings.push(
					createFinding(
						message,
						node,
						rule.meta.id,
						rule.meta.defaultSeverity,
						sourceFile,
					),
				);
			},
		};
		visitors.push(rule.create(ctx));
	}

	// Call onFileStart hooks
	for (const visitor of visitors) {
		visitor.onFileStart?.();
	}

	// Build dispatch table: Map<SyntaxKind, handler[]>
	const dispatch = new Map<ts.SyntaxKind, Array<(node: ts.Node) => void>>();
	for (const visitor of visitors) {
		for (const [key, handler] of Object.entries(visitor)) {
			if (key === "onFileStart") continue;
			const kind = Number(key) as ts.SyntaxKind;
			const handlers = dispatch.get(kind) ?? [];
			handlers.push(handler as (node: ts.Node) => void);
			dispatch.set(kind, handlers);
		}
	}

	// Single-pass traversal with dispatch
	walkNode(sourceFile, dispatch);

	return findings;
}

function walkNode(
	node: ts.Node,
	dispatch: Map<ts.SyntaxKind, Array<(node: ts.Node) => void>>,
): void {
	const handlers = dispatch.get(node.kind);
	if (handlers) {
		for (const handler of handlers) {
			handler(node);
		}
	}

	ts.forEachChild(node, (child) => walkNode(child, dispatch));
}

function createFinding(
	message: string,
	node: ts.Node,
	ruleId: string,
	severity: "block" | "warn" | "off",
	sourceFile: ts.SourceFile,
): Finding {
	const { line, col } = getLineCol(node, sourceFile);
	return {
		ruleId: ruleId as Finding["ruleId"],
		severity,
		message,
		file: sourceFile.fileName,
		line,
		col,
		snippet: node.getText(sourceFile).split("\n")[0].slice(0, 80),
	};
}

function getLineCol(
	node: ts.Node,
	sourceFile: ts.SourceFile,
): { line: number; col: number } {
	const { line, character } = sourceFile.getLineAndCharacterOfPosition(
		node.getStart(sourceFile),
	);
	return { line: line + 1, col: character + 1 };
}
