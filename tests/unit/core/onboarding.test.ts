import { afterEach, describe, expect, test, vi } from "vitest";
import {
  checkOnboardingStatusEffect,
  completeOnboardingEffect,
} from "../../../src/core/onboarding";
import { CLI_USER_AGENT } from "../../../src/shared/cli-trace";
import { runEffect } from "../../setup/effect-test-utils";
import { withValidAuth } from "../../setup/test-utils";

const DEVX_CORE_URL = "https://devx-core.test";

afterEach(() => {
  vi.unstubAllGlobals();
  process.env.DEVX_CORE_URL = undefined;
});

function mockOnboardingFetch(responseBody: unknown) {
  const fetchMock = vi.fn().mockResolvedValue(
    new Response(JSON.stringify(responseBody), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    }),
  );
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

function expectTraceHeaders(fetchMock: ReturnType<typeof vi.fn>) {
  expect(fetchMock).toHaveBeenCalledWith(
    expect.any(String),
    expect.objectContaining({
      headers: expect.objectContaining({
        Authorization: "Bearer test-token-123",
        "Content-Type": "application/json",
        "user-agent": CLI_USER_AGENT,
        "x-request-id": expect.any(String),
      }),
    }),
  );
}

describe("onboarding requests", () => {
  test("sends the CLI User-Agent when checking onboarding status", async () => {
    process.env.DEVX_CORE_URL = DEVX_CORE_URL;
    withValidAuth();
    const fetchMock = mockOnboardingFetch({
      data: { id: "org-123", status: "PENDING" },
    });

    await expect(runEffect(checkOnboardingStatusEffect())).resolves.toEqual({
      orgId: "org-123",
      status: "PENDING",
    });

    expect(fetchMock).toHaveBeenCalledWith(
      `${DEVX_CORE_URL}/api/v1/onboarding/status`,
      expect.any(Object),
    );
    expectTraceHeaders(fetchMock);
  });

  test("sends the CLI User-Agent when completing onboarding", async () => {
    process.env.DEVX_CORE_URL = DEVX_CORE_URL;
    withValidAuth();
    const fetchMock = mockOnboardingFetch({
      data: { organizationId: "org-123", status: "ACTIVE" },
    });

    await expect(runEffect(completeOnboardingEffect())).resolves.toEqual({
      organizationId: "org-123",
      alreadyActive: true,
    });

    expect(fetchMock).toHaveBeenCalledWith(
      `${DEVX_CORE_URL}/api/v1/onboarding/cli`,
      expect.any(Object),
    );
    expectTraceHeaders(fetchMock);
  });
});
