import type { Rule } from "../types.ts";
import { SEC001 } from "./SEC001-eval.ts";
import { SEC002 } from "./SEC002-child-process.ts";
import { SEC003 } from "./SEC003-vm.ts";
import { SEC004 } from "./SEC004-process-internals.ts";
import { SEC005 } from "./SEC005-native-addons.ts";
import { SEC006 } from "./SEC006-module-patching.ts";
import { SEC007 } from "./SEC007-inspector.ts";
import { SEC008 } from "./SEC008-external-urls.ts";
import { SEC009 } from "./SEC009-large-blobs.ts";
import { SEC010 } from "./SEC010-sensitive-paths.ts";

/**
 * All security rules for extension scanning
 *
 * Rules are executed in a single pass over the AST.
 * Each rule defines which SyntaxKind nodes it's interested in.
 */
export const RULES: Rule[] = [
	SEC001, // eval() and Function constructor
	SEC002, // child_process module
	SEC003, // vm module
	SEC004, // process internals
	SEC005, // native addons
	SEC006, // module patching
	SEC007, // inspector module
	SEC008, // external URLs (warn)
	SEC009, // large encoded blobs (warn)
	SEC010, // sensitive paths (warn)
];

/**
 * Get rules by severity
 */
export function getRulesBySeverity(severity: "block" | "warn" | "off"): Rule[] {
	return RULES.filter((rule) => rule.meta.defaultSeverity === severity);
}

/**
 * Get rule by ID
 */
export function getRuleById(id: string): Rule | undefined {
	return RULES.find((rule) => rule.meta.id === id);
}
