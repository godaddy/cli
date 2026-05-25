import { type } from "arktype";
import { describe, expect, test } from "vitest";
import {
  isPublicRoutableUrl,
  publicHttpUrl,
} from "../../../src/services/public-url";

describe("isPublicRoutableUrl", () => {
  describe("accepts publicly-resolvable http(s) URLs", () => {
    test.each([
      "https://example.com",
      "https://app.example.com",
      "http://example.com",
      "https://api.example.com:8443",
      "https://example.com/path?query=1",
      "https://sub.domain.example.co.uk",
      "https://xn--bcher-kva.example.com",
      "https://[2606:4700:4700::1111]",
    ])("accepts %s", (url) => {
      expect(isPublicRoutableUrl(url)).toBe(true);
    });
  });

  describe("rejects localhost variants", () => {
    test.each([
      "http://localhost",
      "http://localhost:3000",
      "http://localhost:3000/callback",
      "https://localhost:5678",
      "http://LOCALHOST",
      "http://foo.localhost",
      "http://foo.local",
      "http://foo.bar.local",
    ])("rejects %s", (url) => {
      expect(isPublicRoutableUrl(url)).toBe(false);
    });
  });

  describe("rejects loopback and unspecified addresses", () => {
    test.each([
      "http://127.0.0.1",
      "http://127.0.0.1:8080",
      "http://127.1.2.3",
      "http://0.0.0.0",
      "http://0.0.0.0:8080",
      "http://[::1]",
      "http://[::1]:8080",
      "http://[::]",
    ])("rejects %s", (url) => {
      expect(isPublicRoutableUrl(url)).toBe(false);
    });
  });

  describe("rejects RFC1918 private IPv4 ranges", () => {
    test.each([
      "http://10.0.0.1",
      "http://10.255.255.255",
      "http://172.16.0.1",
      "http://172.20.1.2",
      "http://172.31.255.255",
      "http://192.168.0.1",
      "http://192.168.1.100",
    ])("rejects %s", (url) => {
      expect(isPublicRoutableUrl(url)).toBe(false);
    });
  });

  describe("rejects link-local ranges", () => {
    test.each([
      "http://169.254.0.1",
      "http://169.254.169.254",
      "http://[fe80::1]",
      "http://[FE80::abcd]",
    ])("rejects %s", (url) => {
      expect(isPublicRoutableUrl(url)).toBe(false);
    });
  });

  describe("rejects non-http(s) schemes and malformed input", () => {
    test.each([
      "ftp://example.com",
      "file:///etc/passwd",
      "javascript:alert(1)",
      "not-a-url",
      "",
      "   ",
    ])("rejects %s", (url) => {
      expect(isPublicRoutableUrl(url)).toBe(false);
    });
  });

  test("does not treat non-RFC1918 /8 ranges like 172.15 or 172.32 as private", () => {
    expect(isPublicRoutableUrl("http://172.15.0.1")).toBe(true);
    expect(isPublicRoutableUrl("http://172.32.0.1")).toBe(true);
  });
});

describe("publicHttpUrl (arktype narrowing)", () => {
  test("parses a valid public URL", () => {
    const result = publicHttpUrl("https://app.example.com");
    expect(result).toBe("https://app.example.com");
  });

  test("produces an ArkError for localhost", () => {
    const result = publicHttpUrl("http://localhost:3000/callback");
    expect(result).toBeInstanceOf(type.errors);
    if (result instanceof type.errors) {
      expect(result.summary).toMatch(/publicly-resolvable/i);
    }
  });

  test("produces an ArkError for loopback IP", () => {
    const result = publicHttpUrl("http://127.0.0.1:8080");
    expect(result).toBeInstanceOf(type.errors);
  });

  test("produces an ArkError for non-http(s) scheme", () => {
    const result = publicHttpUrl("ftp://example.com");
    expect(result).toBeInstanceOf(type.errors);
  });
});
