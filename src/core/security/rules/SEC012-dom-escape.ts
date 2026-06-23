import ts from "typescript";
import type { Rule } from "../types.ts";

const BLOCKED_GLOBAL_PROPERTIES: ReadonlyArray<[string, string]> = [
  ["document", "body"],
  ["document", "documentElement"],
  ["document", "head"],
  ["document", "forms"],
  ["document", "images"],
  ["document", "links"],
  ["document", "scripts"],
  ["document", "cookie"],
  ["document", "activeElement"],
  ["document", "children"],
  ["document", "firstElementChild"],
  ["window", "document"],
  ["window", "location"],
  ["globalThis", "document"],
  ["globalThis", "location"],
  ["location", "href"],
  ["location", "assign"],
  ["location", "replace"],
  ["history", "pushState"],
  ["history", "replaceState"],
  ["top", "document"],
  ["top", "location"],
  ["parent", "document"],
  ["parent", "location"],
  ["Element", "prototype"],
  ["Node", "prototype"],
  ["container", "ownerDocument"],
  ["container", "parentElement"],
  ["container", "parentNode"],
  ["container", "closest"],
];

const BLOCKED_GLOBAL_CALLS: ReadonlyArray<[string, string]> = [
  ["document", "write"],
  ["document", "querySelector"],
  ["document", "querySelectorAll"],
  ["document", "getElementById"],
  ["document", "getElementsByClassName"],
  ["document", "getElementsByName"],
  ["document", "getElementsByTagName"],
  ["document", "getElementsByTagNameNS"],
  ["document", "createElement"],
  ["document", "createRange"],
  ["document", "evaluate"],
  ["window", "open"],
  ["location", "assign"],
  ["location", "replace"],
  ["history", "pushState"],
  ["history", "replaceState"],
  ["container", "closest"],
];

const BLOCKED_NESTED_CALLS: ReadonlyArray<[string, string, string]> = [
  ["window", "document", "querySelector"],
  ["window", "document", "querySelectorAll"],
  ["window", "document", "getElementById"],
  ["window", "document", "getElementsByClassName"],
  ["window", "document", "getElementsByName"],
  ["window", "document", "getElementsByTagName"],
  ["window", "document", "getElementsByTagNameNS"],
  ["window", "document", "write"],
  ["window", "location", "assign"],
  ["window", "location", "replace"],
  ["globalThis", "document", "querySelector"],
  ["globalThis", "document", "querySelectorAll"],
  ["globalThis", "document", "getElementById"],
  ["globalThis", "document", "write"],
  ["globalThis", "location", "assign"],
  ["globalThis", "location", "replace"],
  ["top", "document", "querySelector"],
  ["top", "document", "querySelectorAll"],
  ["top", "document", "getElementById"],
  ["parent", "document", "querySelector"],
  ["parent", "document", "querySelectorAll"],
  ["parent", "document", "getElementById"],
];

const BLOCKED_GLOBAL_IDENTIFIERS = new Set(["localStorage", "sessionStorage"]);

const BLOCKED_GLOBAL_FUNCTIONS = new Set(["open"]);

function isIdentifierNamed(node: ts.Node, name: string): boolean {
  return ts.isIdentifier(node) && node.text === name;
}

function isPropertyAccess(
  node: ts.Node,
  objectName: string,
  propertyName: string,
): boolean {
  return (
    ts.isPropertyAccessExpression(node) &&
    isIdentifierNamed(node.expression, objectName) &&
    isIdentifierNamed(node.name, propertyName)
  );
}

function isNestedPropertyAccess(
  node: ts.Node,
  objectName: string,
  firstPropertyName: string,
  secondPropertyName: string,
): boolean {
  return (
    ts.isPropertyAccessExpression(node) &&
    isIdentifierNamed(node.name, secondPropertyName) &&
    isPropertyAccess(node.expression, objectName, firstPropertyName)
  );
}

function isBlockedPropertyAccess(node: ts.Node): boolean {
  return BLOCKED_GLOBAL_PROPERTIES.some(([objectName, propertyName]) =>
    isPropertyAccess(node, objectName, propertyName),
  );
}

function isBlockedCallExpression(node: ts.CallExpression): boolean {
  const { expression } = node;

  if (
    ts.isIdentifier(expression) &&
    BLOCKED_GLOBAL_FUNCTIONS.has(expression.text)
  ) {
    return true;
  }

  return (
    BLOCKED_GLOBAL_CALLS.some(([objectName, propertyName]) =>
      isPropertyAccess(expression, objectName, propertyName),
    ) ||
    BLOCKED_NESTED_CALLS.some(
      ([objectName, firstPropertyName, secondPropertyName]) =>
        isNestedPropertyAccess(
          expression,
          objectName,
          firstPropertyName,
          secondPropertyName,
        ),
    )
  );
}

/**
 * SEC012: DOM escape operation in UI extension source.
 *
 * Phase 1 DOM bundle extensions must render only inside the host-provided
 * container. This rule blocks obvious page-level DOM and navigation access.
 */
export const SEC012: Rule = {
  meta: {
    id: "SEC012",
    defaultSeverity: "block",
    title: "DOM escape operation in UI extension source",
    description:
      "Blocks page-level DOM, storage, or navigation APIs that can escape the host-provided UI extension container",
    remediation:
      "Render only inside the container passed to mount(). Do not query or mutate checkout page DOM directly.",
  },
  create: (ctx) => {
    return {
      [ts.SyntaxKind.Identifier]: (node: ts.Node) => {
        const identifier = node as ts.Identifier;
        if (BLOCKED_GLOBAL_IDENTIFIERS.has(identifier.text)) {
          ctx.report(
            "Blocked: UI extensions must not access page-global browser storage.",
            node,
          );
        }
      },
      [ts.SyntaxKind.PropertyAccessExpression]: (node: ts.Node) => {
        if (isBlockedPropertyAccess(node)) {
          ctx.report(
            "Blocked: UI extensions must render only inside the host-provided container and must not access page-level DOM, storage, or navigation APIs.",
            node,
          );
        }
      },
      [ts.SyntaxKind.CallExpression]: (node: ts.Node) => {
        const call = node as ts.CallExpression;
        if (isBlockedCallExpression(call)) {
          ctx.report(
            "Blocked: UI extensions must render only inside the host-provided container and must not query, write, navigate, or escape checkout page DOM directly.",
            node,
          );
        }
      },
    };
  },
};
