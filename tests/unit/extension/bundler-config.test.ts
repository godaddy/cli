/**
 * Tests for esbuild configuration generation.
 * Verifies standard build options and override behavior.
 */

import { buildEsbuildOptions } from "@core/extension/bundler-config";
import { describe, expect, it } from "vitest";

describe("buildEsbuildOptions", () => {
	const testEntryPath = "/path/to/extension/src/index.ts";

	it("should generate config with required standard options", () => {
		const config = buildEsbuildOptions(testEntryPath);

		expect(config.bundle).toBe(true);
		// Default (no extensionType) uses browser/iife for embed/checkout compatibility
		expect(config.format).toBe("iife");
		expect(config.platform).toBe("browser");
		expect(config.target).toBe("es2020");
		expect(config.entryPoints).toEqual([testEntryPath]);
	});

	it("should generate ESM config for blocks extensions", () => {
		const config = buildEsbuildOptions({
			entryPath: testEntryPath,
			extensionType: "blocks",
		});

		expect(config.bundle).toBe(true);
		expect(config.format).toBe("esm");
		expect(config.platform).toBe("node");
		expect(config.target).toBe("node22");
	});

	it("should set minify to true", () => {
		const config = buildEsbuildOptions(testEntryPath);

		expect(config.minify).toBe(true);
	});

	it("should set sourcemap to external", () => {
		const config = buildEsbuildOptions(testEntryPath);

		expect(config.sourcemap).toBe("external");
	});

	it("should set write to false", () => {
		const config = buildEsbuildOptions(testEntryPath);

		expect(config.write).toBe(false);
	});

	it("should externalize node built-ins and @wsblocks/* by default", () => {
		const config = buildEsbuildOptions(testEntryPath);

		expect(config.external).toEqual(["node:*", "@wsblocks/*"]);
	});

	it("should externalize React for blocks extensions", () => {
		const config = buildEsbuildOptions({
			entryPath: testEntryPath,
			extensionType: "blocks",
		});

		expect(config.external).toEqual([
			"node:*",
			"@wsblocks/*",
			"react",
			"react/*",
			"react-dom",
			"react-dom/*",
		]);
	});

	it("should not externalize React for embed extensions", () => {
		const config = buildEsbuildOptions({
			entryPath: testEntryPath,
			extensionType: "embed",
		});

		expect(config.external).toEqual(["node:*", "@wsblocks/*"]);
	});

	it("should not externalize React for checkout extensions", () => {
		const config = buildEsbuildOptions({
			entryPath: testEntryPath,
			extensionType: "checkout",
		});

		expect(config.external).toEqual(["node:*", "@wsblocks/*"]);
	});

	it("should set outExtension for .mjs", () => {
		const config = buildEsbuildOptions(testEntryPath);

		expect(config.outExtension).toEqual({ ".js": ".mjs" });
	});

	it("should include tsconfig when provided", () => {
		const tsconfigPath = "/path/to/tsconfig.json";
		const config = buildEsbuildOptions(testEntryPath, tsconfigPath);

		expect(config.tsconfig).toBe(tsconfigPath);
	});

	it("should omit tsconfig when not provided", () => {
		const config = buildEsbuildOptions(testEntryPath);

		expect(config.tsconfig).toBeUndefined();
	});

	it("should apply overrides to standard config", () => {
		const overrides = {
			minify: false,
			target: "node20" as const,
		};
		const config = buildEsbuildOptions(testEntryPath, undefined, overrides);

		expect(config.minify).toBe(false);
		expect(config.target).toBe("node20");
		// Verify other standard options remain unchanged
		expect(config.bundle).toBe(true);
		// Default (no extensionType) uses browser/iife
		expect(config.format).toBe("iife");
		expect(config.platform).toBe("browser");
	});

	it("should set logLevel to silent", () => {
		const config = buildEsbuildOptions(testEntryPath);

		expect(config.logLevel).toBe("silent");
	});
});
