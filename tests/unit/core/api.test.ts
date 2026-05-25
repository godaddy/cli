import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import {
  apiRequestEffect,
  parseFieldsEffect,
  parseHeadersEffect,
  readBodyFromFileEffect,
} from "../../../src/core/api";
import {
  extractFailure,
  runEffect,
  runEffectExit,
} from "../../setup/effect-test-utils";
import { mockKeytar, mockValidToken } from "../../setup/system-mocks";

describe("API Core Functions", () => {
  beforeEach(() => {
    mockValidToken();
    process.env.GODADDY_API_BASE_URL = "";
    vi.stubGlobal("fetch", vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    process.env.GODADDY_API_BASE_URL = "";
  });

  describe("apiRequestEffect", () => {
    test("returns auth error when secure credential storage is unavailable", async () => {
      mockKeytar.getPassword.mockRejectedValueOnce(
        new Error("Keychain locked"),
      );

      const exit = await runEffectExit(
        apiRequestEffect({ endpoint: "/v1/domains" }),
      );
      const err = extractFailure(exit) as {
        _tag: string;
        userMessage: string;
      };
      expect(err._tag).toBe("AuthenticationError");
      expect(err.userMessage).toContain("Unable to access secure credentials");
      expect(fetch).not.toHaveBeenCalled();
    });

    test("returns validation error for full URL endpoints", async () => {
      const exit = await runEffectExit(
        apiRequestEffect({
          endpoint: "https://api.godaddy.com/v1/domains",
        }),
      );
      const err = extractFailure(exit) as {
        _tag: string;
        userMessage: string;
      };
      expect(err._tag).toBe("ValidationError");
      expect(err.userMessage).toContain("Only relative endpoints");
      expect(fetch).not.toHaveBeenCalled();
    });

    test("makes authenticated request and returns parsed JSON", async () => {
      vi.mocked(fetch).mockResolvedValueOnce(
        new Response(JSON.stringify({ shopperId: "12345" }), {
          status: 200,
          headers: {
            "content-type": "application/json",
            "x-request-id": "resp-123",
          },
        }),
      );

      const result = await runEffect(
        apiRequestEffect({ endpoint: "/v1/shoppers/me" }),
      );

      expect(result.status).toBe(200);
      expect(result.data).toEqual({ shopperId: "12345" });
      expect(fetch).toHaveBeenCalledTimes(1);
      expect(fetch).toHaveBeenCalledWith(
        "https://api.ote-godaddy.com/v1/shoppers/me",
        expect.objectContaining({
          method: "GET",
          headers: expect.objectContaining({
            Authorization: "Bearer test-token-123",
            "x-request-id": expect.any(String),
            "user-agent": expect.stringMatching(/^godaddy-cli\/\d+\.\d+\.\d+$/),
          }),
        }),
      );
    });

    test("returns auth error on 401 response", async () => {
      vi.mocked(fetch).mockResolvedValueOnce(
        new Response(JSON.stringify({ message: "Unauthorized" }), {
          status: 401,
          headers: { "content-type": "application/json" },
        }),
      );

      const exit = await runEffectExit(
        apiRequestEffect({ endpoint: "/v1/shoppers/me" }),
      );
      const err = extractFailure(exit) as {
        _tag: string;
        userMessage: string;
      };
      expect(err._tag).toBe("AuthenticationError");
      expect(err.userMessage).toContain("re-authenticate");
    });

    test("returns network error when graphql response contains errors", async () => {
      vi.mocked(fetch).mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            data: { sku: null },
            errors: [{ message: "Cannot query field 'skuX' on type 'Query'." }],
          }),
          {
            status: 200,
            statusText: "OK",
            headers: {
              "content-type": "application/json",
              "x-request-id": "graphql-req-1",
            },
          },
        ),
      );

      const exit = await runEffectExit(
        apiRequestEffect({
          endpoint: "/v2/commerce/stores/test/catalog/graphql",
          method: "POST",
          body: JSON.stringify({ query: "{ skuX { id } }" }),
          graphql: true,
        }),
      );

      const err = extractFailure(exit) as {
        _tag: string;
        userMessage: string;
        status?: number;
        requestId?: string;
        responseBody?: unknown;
      };

      expect(err._tag).toBe("NetworkError");
      expect(err.userMessage).toContain("GraphQL request returned errors");
      expect(err.userMessage).toContain("Cannot query field");
      expect(err.status).toBe(200);
      expect(err.requestId).toBe("graphql-req-1");
      expect(err.responseBody).toEqual(
        expect.objectContaining({
          errors: expect.arrayContaining([
            expect.objectContaining({
              message: "Cannot query field 'skuX' on type 'Query'.",
            }),
          ]),
        }),
      );
    });

    test("returns structured network error details for 400 responses", async () => {
      vi.mocked(fetch).mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            code: "VALIDATION_FAILED",
            message: "Validation error",
            fields: [{ path: "lineItems[0].sku", message: "Required" }],
          }),
          {
            status: 400,
            statusText: "Bad Request",
            headers: {
              "content-type": "application/json",
              "godaddy-request-id": "req-123",
            },
          },
        ),
      );

      const exit = await runEffectExit(
        apiRequestEffect({
          endpoint: "/v1/commerce/stores/test-store/orders",
          method: "POST",
          body: "{}",
        }),
      );

      const err = extractFailure(exit) as {
        _tag: string;
        userMessage: string;
        status?: number;
        statusText?: string;
        endpoint?: string;
        method?: string;
        requestId?: string;
        responseBody?: unknown;
      };

      expect(err._tag).toBe("NetworkError");
      expect(err.userMessage).toContain("API request rejected (400)");
      expect(err.userMessage).toContain("Validation error");
      expect(err.status).toBe(400);
      expect(err.statusText).toBe("Bad Request");
      expect(err.endpoint).toBe("/v1/commerce/stores/test-store/orders");
      expect(err.method).toBe("POST");
      expect(err.requestId).toBe("req-123");
      expect(err.responseBody).toEqual(
        expect.objectContaining({
          code: "VALIDATION_FAILED",
          message: "Validation error",
        }),
      );
    });
  });

  describe("parseFieldsEffect", () => {
    test("parses single field correctly", async () => {
      const result = await runEffect(parseFieldsEffect(["name=John"]));
      expect(result).toEqual({ name: "John" });
    });

    test("parses multiple fields correctly", async () => {
      const result = await runEffect(
        parseFieldsEffect(["name=John", "age=30", "city=NYC"]),
      );
      expect(result).toEqual({ name: "John", age: "30", city: "NYC" });
    });

    test("handles values with equals signs", async () => {
      const result = await runEffect(parseFieldsEffect(["query=a=b&c=d"]));
      expect(result).toEqual({ query: "a=b&c=d" });
    });

    test("handles empty value", async () => {
      const result = await runEffect(parseFieldsEffect(["key="]));
      expect(result).toEqual({ key: "" });
    });

    test("returns error for missing equals sign", async () => {
      const exit = await runEffectExit(parseFieldsEffect(["invalidfield"]));
      const err = extractFailure(exit) as { userMessage: string };
      expect(err.userMessage).toContain("Invalid field format");
    });

    test("returns error for empty key", async () => {
      const exit = await runEffectExit(parseFieldsEffect(["=value"]));
      const err = extractFailure(exit) as { userMessage: string };
      expect(err.userMessage).toContain("Empty field key");
    });

    test("handles empty array", async () => {
      const result = await runEffect(parseFieldsEffect([]));
      expect(result).toEqual({});
    });
  });

  describe("parseHeadersEffect", () => {
    test("parses single header correctly", async () => {
      const result = await runEffect(
        parseHeadersEffect(["Content-Type: application/json"]),
      );
      expect(result).toEqual({ "Content-Type": "application/json" });
    });

    test("parses multiple headers correctly", async () => {
      const result = await runEffect(
        parseHeadersEffect([
          "Content-Type: application/json",
          "X-Custom: value",
          "Accept: */*",
        ]),
      );
      expect(result).toEqual({
        "Content-Type": "application/json",
        "X-Custom": "value",
        Accept: "*/*",
      });
    });

    test("handles header values with colons", async () => {
      const result = await runEffect(parseHeadersEffect(["X-Time: 12:30:00"]));
      expect(result).toEqual({ "X-Time": "12:30:00" });
    });

    test("trims whitespace from key and value", async () => {
      const result = await runEffect(
        parseHeadersEffect(["  Content-Type  :  application/json  "]),
      );
      expect(result).toEqual({ "Content-Type": "application/json" });
    });

    test("returns error for missing colon", async () => {
      const exit = await runEffectExit(parseHeadersEffect(["InvalidHeader"]));
      const err = extractFailure(exit) as { userMessage: string };
      expect(err.userMessage).toContain("Invalid header format");
    });

    test("returns error for empty key", async () => {
      const exit = await runEffectExit(parseHeadersEffect([": value"]));
      const err = extractFailure(exit) as { userMessage: string };
      expect(err.userMessage).toContain("Empty header key");
    });

    test("handles empty array", async () => {
      const result = await runEffect(parseHeadersEffect([]));
      expect(result).toEqual({});
    });
  });

  describe("readBodyFromFileEffect", () => {
    test("returns error for non-existent file", async () => {
      const exit = await runEffectExit(
        readBodyFromFileEffect("/non/existent/file.json"),
      );
      const err = extractFailure(exit) as { userMessage: string };
      expect(err.userMessage).toContain("File not found");
    });
  });
});
