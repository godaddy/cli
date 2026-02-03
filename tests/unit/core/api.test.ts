import { describe, expect, test } from "vitest";
import {
	parseFields,
	parseHeaders,
	readBodyFromFile,
} from "../../../src/core/api";

describe("API Core Functions", () => {
	describe("parseFields", () => {
		test("parses single field correctly", () => {
			const result = parseFields(["name=John"]);
			expect(result.success).toBe(true);
			expect(result.data).toEqual({ name: "John" });
		});

		test("parses multiple fields correctly", () => {
			const result = parseFields(["name=John", "age=30", "city=NYC"]);
			expect(result.success).toBe(true);
			expect(result.data).toEqual({ name: "John", age: "30", city: "NYC" });
		});

		test("handles values with equals signs", () => {
			const result = parseFields(["query=a=b&c=d"]);
			expect(result.success).toBe(true);
			expect(result.data).toEqual({ query: "a=b&c=d" });
		});

		test("handles empty value", () => {
			const result = parseFields(["key="]);
			expect(result.success).toBe(true);
			expect(result.data).toEqual({ key: "" });
		});

		test("returns error for missing equals sign", () => {
			const result = parseFields(["invalidfield"]);
			expect(result.success).toBe(false);
			expect(result.error?.userMessage).toContain("Invalid field format");
		});

		test("returns error for empty key", () => {
			const result = parseFields(["=value"]);
			expect(result.success).toBe(false);
			expect(result.error?.userMessage).toContain("Empty field key");
		});

		test("handles empty array", () => {
			const result = parseFields([]);
			expect(result.success).toBe(true);
			expect(result.data).toEqual({});
		});
	});

	describe("parseHeaders", () => {
		test("parses single header correctly", () => {
			const result = parseHeaders(["Content-Type: application/json"]);
			expect(result.success).toBe(true);
			expect(result.data).toEqual({ "Content-Type": "application/json" });
		});

		test("parses multiple headers correctly", () => {
			const result = parseHeaders([
				"Content-Type: application/json",
				"X-Custom: value",
				"Accept: */*",
			]);
			expect(result.success).toBe(true);
			expect(result.data).toEqual({
				"Content-Type": "application/json",
				"X-Custom": "value",
				Accept: "*/*",
			});
		});

		test("handles header values with colons", () => {
			const result = parseHeaders(["X-Time: 12:30:00"]);
			expect(result.success).toBe(true);
			expect(result.data).toEqual({ "X-Time": "12:30:00" });
		});

		test("trims whitespace from key and value", () => {
			const result = parseHeaders(["  Content-Type  :  application/json  "]);
			expect(result.success).toBe(true);
			expect(result.data).toEqual({ "Content-Type": "application/json" });
		});

		test("returns error for missing colon", () => {
			const result = parseHeaders(["InvalidHeader"]);
			expect(result.success).toBe(false);
			expect(result.error?.userMessage).toContain("Invalid header format");
		});

		test("returns error for empty key", () => {
			const result = parseHeaders([": value"]);
			expect(result.success).toBe(false);
			expect(result.error?.userMessage).toContain("Empty header key");
		});

		test("handles empty array", () => {
			const result = parseHeaders([]);
			expect(result.success).toBe(true);
			expect(result.data).toEqual({});
		});
	});

	describe("readBodyFromFile", () => {
		test("returns error for non-existent file", () => {
			const result = readBodyFromFile("/non/existent/file.json");
			expect(result.success).toBe(false);
			expect(result.error?.userMessage).toContain("File not found");
		});
	});
});
