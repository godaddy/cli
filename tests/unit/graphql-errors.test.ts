import { HttpResponse, graphql } from "msw";
import { describe, expect, test } from "vitest";
import { getApplication } from "../../src/services/applications";
import { server } from "../setup/msw-server";
import { withNoAuth, withValidAuth } from "../setup/test-utils";

describe("GraphQL Error Handling", () => {
	test("handles authentication error", async () => {
		withNoAuth();

		await expect(
			getApplication("test-app-1", { accessToken: null }),
		).rejects.toThrow("Access token is required");
	});

	test("handles validation errors", async () => {
		withValidAuth();

		// Override handler to return validation error
		server.use(
			graphql.operation(() => {
				return HttpResponse.json(
					{
						data: null,
						errors: [
							{
								message: "Validation failed",
								extensions: {
									code: "VALIDATION_ERROR",
									fieldErrors: {
										name: ["Name is required"],
									},
								},
							},
						],
					},
					{ status: 200 },
				);
			}),
		);

		await expect(
			getApplication("", { accessToken: "test-token-123" }),
		).rejects.toThrow("Validation failed");
	});

	test("handles server errors", async () => {
		withValidAuth();

		server.use(
			graphql.operation(() => {
				return HttpResponse.json(
					{
						data: null,
						errors: [{ message: "Internal server error" }],
					},
					{ status: 200 },
				);
			}),
		);

		await expect(
			getApplication("test-app-1", { accessToken: "test-token-123" }),
		).rejects.toThrow("Internal server error");
	});

	test("handles network errors", async () => {
		withValidAuth();

		server.use(
			graphql.operation(() => {
				return HttpResponse.error();
			}),
		);

		await expect(
			getApplication("test-app-1", { accessToken: "test-token-123" }),
		).rejects.toThrow();
	});
});
