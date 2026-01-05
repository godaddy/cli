import {
	getSecurityConfig,
	isTrustedDomain,
	shouldExcludeFile,
} from "@/core/security/config.ts";
import { describe, expect, it } from "vitest";

describe("Security Configuration", () => {
	describe("getSecurityConfig", () => {
		it("should return strict mode configuration", () => {
			const config = getSecurityConfig();

			expect(config.mode).toBe("strict");
		});

		it("should include GoDaddy trusted domains", () => {
			const config = getSecurityConfig();

			expect(config.trustedDomains).toContain("*.godaddy.com");
		});

		it("should include localhost and 127.0.0.1 as trusted domains", () => {
			const config = getSecurityConfig();

			expect(config.trustedDomains).toContain("localhost");
			expect(config.trustedDomains).toContain("127.0.0.1");
		});

		it("should include standard exclusion patterns", () => {
			const config = getSecurityConfig();

			expect(config.exclude).toContain("**/node_modules/**");
			expect(config.exclude).toContain("**/dist/**");
			expect(config.exclude).toContain("**/build/**");
			expect(config.exclude).toContain("**/__tests__/**");
		});

		it("should return immutable configuration (same object each time)", () => {
			const config1 = getSecurityConfig();
			const config2 = getSecurityConfig();

			expect(config1).toBe(config2);
		});
	});

	describe("isTrustedDomain", () => {
		const config = getSecurityConfig();

		it("should accept exact domain matches", () => {
			expect(isTrustedDomain("localhost", config)).toBe(true);
			expect(isTrustedDomain("127.0.0.1", config)).toBe(true);
			expect(isTrustedDomain("godaddy.com", config)).toBe(true);
		});

		it("should support wildcard subdomain patterns", () => {
			expect(isTrustedDomain("api.godaddy.com", config)).toBe(true);
			expect(isTrustedDomain("www.godaddy.com", config)).toBe(true);
			expect(isTrustedDomain("developer.godaddy.com", config)).toBe(true);
		});

		it("should accept localhost", () => {
			expect(isTrustedDomain("localhost", config)).toBe(true);
		});

		it("should accept 127.0.0.1", () => {
			expect(isTrustedDomain("127.0.0.1", config)).toBe(true);
		});

		it("should reject untrusted domains", () => {
			expect(isTrustedDomain("evil.com", config)).toBe(false);
			expect(isTrustedDomain("malicious.net", config)).toBe(false);
			expect(isTrustedDomain("attacker.org", config)).toBe(false);
		});

		it("should handle subdomains correctly with wildcard", () => {
			expect(isTrustedDomain("api.dev.godaddy.com", config)).toBe(true);
			expect(isTrustedDomain("deep.nested.godaddy.com", config)).toBe(true);
		});

		it("should not match partial domain names", () => {
			expect(isTrustedDomain("notgodaddy.com", config)).toBe(false);
			expect(isTrustedDomain("godaddy.com.evil.com", config)).toBe(false);
			expect(isTrustedDomain("fakegodaddy.com", config)).toBe(false);
		});

		it("should handle domain with port numbers", () => {
			expect(isTrustedDomain("localhost:3000", config)).toBe(true);
			expect(isTrustedDomain("127.0.0.1:8080", config)).toBe(true);
			expect(isTrustedDomain("api.godaddy.com:443", config)).toBe(true);
		});

		it("should be case-insensitive", () => {
			expect(isTrustedDomain("API.GODADDY.COM", config)).toBe(true);
			expect(isTrustedDomain("LOCALHOST", config)).toBe(true);
		});
	});

	describe("shouldExcludeFile", () => {
		const config = getSecurityConfig();

		it("should exclude node_modules files", () => {
			expect(
				shouldExcludeFile("/path/to/node_modules/pkg/index.js", config),
			).toBe(true);
			expect(
				shouldExcludeFile("/project/node_modules/react/index.js", config),
			).toBe(true);
		});

		it("should exclude dist directory files", () => {
			expect(shouldExcludeFile("/path/to/dist/bundle.js", config)).toBe(true);
			expect(shouldExcludeFile("/project/dist/output.min.js", config)).toBe(
				true,
			);
		});

		it("should exclude build directory files", () => {
			expect(shouldExcludeFile("/path/to/build/output.js", config)).toBe(true);
			expect(shouldExcludeFile("/project/build/app.js", config)).toBe(true);
		});

		it("should exclude test files", () => {
			expect(shouldExcludeFile("/path/to/__tests__/unit.test.ts", config)).toBe(
				true,
			);
			expect(
				shouldExcludeFile("/project/__tests__/integration.test.ts", config),
			).toBe(true);
		});

		it("should not exclude source files", () => {
			expect(shouldExcludeFile("/path/to/src/index.ts", config)).toBe(false);
			expect(shouldExcludeFile("/project/lib/utils.ts", config)).toBe(false);
			expect(shouldExcludeFile("/app/components/Button.tsx", config)).toBe(
				false,
			);
		});

		it("should handle absolute paths", () => {
			expect(
				shouldExcludeFile(
					"/usr/local/project/node_modules/pkg/index.js",
					config,
				),
			).toBe(true);
			expect(shouldExcludeFile("/home/user/app/src/main.ts", config)).toBe(
				false,
			);
		});

		it("should handle relative paths", () => {
			expect(shouldExcludeFile("node_modules/pkg/index.js", config)).toBe(true);
			expect(shouldExcludeFile("src/index.ts", config)).toBe(false);
			expect(shouldExcludeFile("./dist/bundle.js", config)).toBe(true);
		});

		it("should match nested exclusion patterns", () => {
			expect(
				shouldExcludeFile("deeply/nested/node_modules/file.js", config),
			).toBe(true);
			expect(shouldExcludeFile("src/components/dist/bundle.js", config)).toBe(
				true,
			);
			expect(shouldExcludeFile("project/sub/build/output.js", config)).toBe(
				true,
			);
		});

		it("should handle Windows-style paths", () => {
			expect(
				shouldExcludeFile(
					"C:\\Users\\project\\node_modules\\pkg\\index.js",
					config,
				),
			).toBe(true);
			expect(
				shouldExcludeFile("C:\\Users\\project\\src\\index.ts", config),
			).toBe(false);
		});
	});
});
