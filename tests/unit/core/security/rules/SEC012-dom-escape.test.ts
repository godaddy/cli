import { buildAliasMaps } from "@/core/security/alias-builder.ts";
import { scanFile } from "@/core/security/engine.ts";
import { SEC012 } from "@/core/security/rules/SEC012-dom-escape.ts";
import type { SecurityConfig } from "@/core/security/types.ts";
import ts from "typescript";
import { describe, expect, it } from "vitest";

function createSourceFile(code: string): ts.SourceFile {
  return ts.createSourceFile(
    "test.ts",
    code,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS,
  );
}

function scan(code: string) {
  const sourceFile = createSourceFile(code);
  const aliasMaps = buildAliasMaps(sourceFile);
  const config: SecurityConfig = {
    mode: "strict",
    trustedDomains: ["*.godaddy.com"],
    exclude: [],
  };

  return scanFile("test.ts", code, [SEC012], config, aliasMaps);
}

describe("SEC012: DOM escape operation in UI extension source", () => {
  it("blocks page-level DOM access", () => {
    const findings = scan(`
      document.body.innerHTML = "unsafe";
      document.documentElement.className = "unsafe";
      document.head.appendChild(script);
      document.cookie = "unsafe=true";
      document.write("unsafe");
      document.querySelector("#checkout-root");
      document.querySelectorAll("button");
      document.getElementById("checkout-root");
      document.getElementsByClassName("checkout");
      document.getElementsByName("payment");
      document.getElementsByTagName("form");
      document.createElement("script");
      document.createRange();
      document.evaluate("//form", document);
      window.document.querySelector("#checkout-root");
      globalThis.document.getElementById("checkout-root");
    `);

    expect(findings.length).toBeGreaterThanOrEqual(16);
    expect(findings.every((finding) => finding.ruleId === "SEC012")).toBe(true);
    expect(findings.every((finding) => finding.severity === "block")).toBe(
      true,
    );
  });

  it("blocks navigation, storage, and prototype mutation APIs", () => {
    const findings = scan(`
      window.location.href = "https://example.com";
      window.location.assign("https://example.com");
      location.href = "https://example.com";
      location.replace("https://example.com");
      globalThis.location.assign("https://example.com");
      history.pushState({}, "", "/unsafe");
      history.replaceState({}, "", "/unsafe");
      window.open("https://example.com");
      open("https://example.com");
      localStorage.setItem("token", "unsafe");
      sessionStorage.setItem("token", "unsafe");
      top.document.body.innerHTML = "unsafe";
      parent.location.href = "https://example.com";
      Element.prototype.remove = function () {};
      Node.prototype.appendChild = function () {};
    `);

    expect(findings.length).toBeGreaterThanOrEqual(15);
  });

  it("blocks container escape paths", () => {
    const findings = scan(`
      export function mount({ container }) {
        container.ownerDocument.body.innerHTML = "unsafe";
        container.parentElement?.remove();
        container.parentNode?.removeChild(container);
        container.closest("#checkout-root")?.remove();
      }
    `);

    expect(findings.length).toBeGreaterThanOrEqual(4);
  });

  it("allows rendering through the provided container", () => {
    const findings = scan(`
      export function mount({ container }) {
        container.innerHTML = "Extension rendered successfully";
      }
    `);

    expect(findings).toEqual([]);
  });
});
