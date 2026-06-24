import ts from "typescript";
import type { Rule } from "../types.ts";

type StaticMemberAccess =
  | ts.PropertyAccessExpression
  | ts.ElementAccessExpression;

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

const STORAGE_ROOTS = new Set(["localStorage", "sessionStorage"]);

const BLOCKED_GLOBAL_FUNCTIONS = new Set(["open"]);

const ALIASABLE_GLOBAL_ROOTS = new Set([
  "document",
  "location",
  "history",
  "top",
  "parent",
  "Element",
  "Node",
  "container",
  "localStorage",
  "sessionStorage",
  "open",
]);

const DOCUMENT_OWNER_ROOTS = new Set(["window", "globalThis", "top", "parent"]);

function isStaticMemberAccess(node: ts.Node): node is StaticMemberAccess {
  return (
    ts.isPropertyAccessExpression(node) || ts.isElementAccessExpression(node)
  );
}

function getStaticMemberName(node: StaticMemberAccess): string | undefined {
  if (ts.isPropertyAccessExpression(node)) {
    return node.name.text;
  }

  const { argumentExpression } = node;
  if (
    ts.isStringLiteral(argumentExpression) ||
    ts.isNoSubstitutionTemplateLiteral(argumentExpression)
  ) {
    return argumentExpression.text;
  }

  return undefined;
}

function bindingNameContains(
  name: ts.BindingName,
  targetName: string,
): boolean {
  if (ts.isIdentifier(name)) {
    return name.text === targetName;
  }

  return name.elements.some(
    (element) =>
      ts.isBindingElement(element) &&
      bindingNameContains(element.name, targetName),
  );
}

function nodeContains(parent: ts.Node, child: ts.Node): boolean {
  let current: ts.Node | undefined = child;
  while (current) {
    if (current === parent) {
      return true;
    }
    current = current.parent;
  }
  return false;
}

function isIdentifierDeclarationName(node: ts.Identifier): boolean {
  const { parent } = node;

  return (
    (ts.isVariableDeclaration(parent) && parent.name === node) ||
    (ts.isParameter(parent) && parent.name === node) ||
    (ts.isBindingElement(parent) && parent.name === node) ||
    (ts.isFunctionDeclaration(parent) && parent.name === node) ||
    (ts.isFunctionExpression(parent) && parent.name === node) ||
    (ts.isClassDeclaration(parent) && parent.name === node) ||
    (ts.isClassExpression(parent) && parent.name === node) ||
    (ts.isImportClause(parent) && parent.name === node) ||
    (ts.isNamespaceImport(parent) && parent.name === node) ||
    (ts.isImportSpecifier(parent) && parent.name === node) ||
    (ts.isPropertyAccessExpression(parent) && parent.name === node) ||
    (ts.isPropertyAssignment(parent) && parent.name === node)
  );
}

function importClauseDeclaresName(
  importClause: ts.ImportClause,
  targetName: string,
): boolean {
  if (importClause.name?.text === targetName) {
    return true;
  }

  const { namedBindings } = importClause;
  if (!namedBindings) {
    return false;
  }

  if (ts.isNamespaceImport(namedBindings)) {
    return namedBindings.name.text === targetName;
  }

  return namedBindings.elements.some(
    (element) => element.name.text === targetName,
  );
}

function statementDeclaresName(
  statement: ts.Statement,
  targetName: string,
  reference: ts.Node,
): boolean {
  if (ts.isVariableStatement(statement)) {
    return statement.declarationList.declarations.some(
      (declaration) =>
        bindingNameContains(declaration.name, targetName) &&
        !nodeContains(declaration.name, reference),
    );
  }

  if (
    ts.isFunctionDeclaration(statement) &&
    statement.name?.text === targetName &&
    !nodeContains(statement.name, reference)
  ) {
    return true;
  }

  if (
    ts.isClassDeclaration(statement) &&
    statement.name?.text === targetName &&
    !nodeContains(statement.name, reference)
  ) {
    return true;
  }

  if (ts.isImportDeclaration(statement) && statement.importClause) {
    return importClauseDeclaresName(statement.importClause, targetName);
  }

  return false;
}

function scopeDeclaresName(
  scope: ts.Node,
  targetName: string,
  reference: ts.Node,
): boolean {
  if (
    ts.isFunctionDeclaration(scope) ||
    ts.isFunctionExpression(scope) ||
    ts.isArrowFunction(scope) ||
    ts.isMethodDeclaration(scope) ||
    ts.isConstructorDeclaration(scope) ||
    ts.isGetAccessor(scope) ||
    ts.isSetAccessor(scope)
  ) {
    if (
      scope.parameters.some(
        (parameter) =>
          bindingNameContains(parameter.name, targetName) &&
          !nodeContains(parameter.name, reference),
      )
    ) {
      return true;
    }
  }

  if (ts.isSourceFile(scope) || ts.isBlock(scope) || ts.isModuleBlock(scope)) {
    return scope.statements.some((statement) =>
      statementDeclaresName(statement, targetName, reference),
    );
  }

  return false;
}

function isShadowedGlobalReference(identifier: ts.Identifier): boolean {
  let current: ts.Node | undefined = identifier.parent;
  while (current) {
    if (scopeDeclaresName(current, identifier.text, identifier)) {
      return true;
    }
    current = current.parent;
  }

  return false;
}

function isMemberObjectIdentifier(node: ts.Identifier): boolean {
  const { parent } = node;
  return (
    (ts.isPropertyAccessExpression(parent) && parent.expression === node) ||
    (ts.isElementAccessExpression(parent) && parent.expression === node)
  );
}

function resolveIdentifierRoot(
  node: ts.Identifier,
  aliases: ReadonlyMap<string, string>,
): string | undefined {
  if (isIdentifierDeclarationName(node)) {
    return undefined;
  }

  const aliasRoot = aliases.get(node.text);
  if (aliasRoot) {
    return aliasRoot;
  }

  if (node.text === "container") {
    return "container";
  }

  if (isShadowedGlobalReference(node)) {
    return undefined;
  }

  return node.text;
}

function resolveExpressionRoot(
  node: ts.Node,
  aliases: ReadonlyMap<string, string>,
): string | undefined {
  if (ts.isIdentifier(node)) {
    return resolveIdentifierRoot(node, aliases);
  }

  if (!isStaticMemberAccess(node)) {
    return undefined;
  }

  const memberName = getStaticMemberName(node);
  const ownerRoot = resolveExpressionRoot(node.expression, aliases);

  if (memberName === "document" && DOCUMENT_OWNER_ROOTS.has(ownerRoot ?? "")) {
    return "document";
  }

  if (memberName === "location" && DOCUMENT_OWNER_ROOTS.has(ownerRoot ?? "")) {
    return "location";
  }

  return undefined;
}

function isMemberAccess(
  node: ts.Node,
  objectName: string,
  propertyName: string,
  aliases: ReadonlyMap<string, string>,
): boolean {
  return (
    isStaticMemberAccess(node) &&
    getStaticMemberName(node) === propertyName &&
    resolveExpressionRoot(node.expression, aliases) === objectName
  );
}

function isNestedMemberAccess(
  node: ts.Node,
  objectName: string,
  firstPropertyName: string,
  secondPropertyName: string,
  aliases: ReadonlyMap<string, string>,
): boolean {
  if (
    !isStaticMemberAccess(node) ||
    getStaticMemberName(node) !== secondPropertyName
  ) {
    return false;
  }

  return isMemberAccess(
    node.expression,
    objectName,
    firstPropertyName,
    aliases,
  );
}

function isBlockedPropertyAccess(
  node: ts.Node,
  aliases: ReadonlyMap<string, string>,
): boolean {
  if (!isStaticMemberAccess(node)) {
    return false;
  }

  const objectRoot = resolveExpressionRoot(node.expression, aliases);
  if (STORAGE_ROOTS.has(objectRoot ?? "")) {
    return true;
  }

  return BLOCKED_GLOBAL_PROPERTIES.some(([objectName, propertyName]) =>
    isMemberAccess(node, objectName, propertyName, aliases),
  );
}

function isBlockedCallExpression(
  node: ts.CallExpression,
  aliases: ReadonlyMap<string, string>,
): boolean {
  const { expression } = node;

  if (ts.isIdentifier(expression)) {
    const rootName = resolveIdentifierRoot(expression, aliases);
    if (rootName && BLOCKED_GLOBAL_FUNCTIONS.has(rootName)) {
      return true;
    }
  }

  return (
    BLOCKED_GLOBAL_CALLS.some(([objectName, propertyName]) =>
      isMemberAccess(expression, objectName, propertyName, aliases),
    ) ||
    BLOCKED_NESTED_CALLS.some(
      ([objectName, firstPropertyName, secondPropertyName]) =>
        isNestedMemberAccess(
          expression,
          objectName,
          firstPropertyName,
          secondPropertyName,
          aliases,
        ),
    )
  );
}

function isBlockedStorageIdentifier(
  node: ts.Node,
  aliases: ReadonlyMap<string, string>,
): boolean {
  if (!ts.isIdentifier(node) || isMemberObjectIdentifier(node)) {
    return false;
  }

  const rootName = resolveIdentifierRoot(node, aliases);
  return STORAGE_ROOTS.has(rootName ?? "");
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
    const aliases = new Map<string, string>();

    return {
      [ts.SyntaxKind.VariableDeclaration]: (node: ts.Node) => {
        if (
          !ts.isVariableDeclaration(node) ||
          !ts.isIdentifier(node.name) ||
          !node.initializer
        ) {
          return;
        }

        const rootName = resolveExpressionRoot(node.initializer, aliases);
        if (rootName && ALIASABLE_GLOBAL_ROOTS.has(rootName)) {
          aliases.set(node.name.text, rootName);
        }
      },
      [ts.SyntaxKind.Identifier]: (node: ts.Node) => {
        if (isBlockedStorageIdentifier(node, aliases)) {
          ctx.report(
            "Blocked: UI extensions must not access page-global browser storage.",
            node,
          );
        }
      },
      [ts.SyntaxKind.PropertyAccessExpression]: (node: ts.Node) => {
        if (isBlockedPropertyAccess(node, aliases)) {
          ctx.report(
            "Blocked: UI extensions must render only inside the host-provided container and must not access page-level DOM, storage, or navigation APIs.",
            node,
          );
        }
      },
      [ts.SyntaxKind.ElementAccessExpression]: (node: ts.Node) => {
        if (isBlockedPropertyAccess(node, aliases)) {
          ctx.report(
            "Blocked: UI extensions must render only inside the host-provided container and must not access page-level DOM, storage, or navigation APIs.",
            node,
          );
        }
      },
      [ts.SyntaxKind.CallExpression]: (node: ts.Node) => {
        if (
          ts.isCallExpression(node) &&
          isBlockedCallExpression(node, aliases)
        ) {
          ctx.report(
            "Blocked: UI extensions must render only inside the host-provided container and must not query, write, navigate, or escape checkout page DOM directly.",
            node,
          );
        }
      },
    };
  },
};
