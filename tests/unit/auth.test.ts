import net from "node:net";
import { afterEach, describe, expect, test, vi } from "vitest";
import {
  authLoginEffect,
  getFromKeychainEffect,
  stopAuthServer,
} from "../../src/core/auth";
import { setRuntimeEnvironmentOverride } from "../../src/core/environment";
import { runEffect } from "../setup/effect-test-utils";
import {
  mockKeytar,
  mockOpen,
  setupTestEnvironment,
} from "../setup/system-mocks";
import {
  withExpiredAuth,
  withNoAuth,
  withValidAuth,
} from "../setup/test-utils";

function rawHttpGet(url: URL): Promise<string> {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection(
      { host: "127.0.0.1", port: Number(url.port) },
      () => {
        socket.write(
          `GET ${url.pathname}${url.search} HTTP/1.1\r\nHost: ${url.host}\r\nConnection: close\r\n\r\n`,
        );
      },
    );
    let response = "";
    socket.setEncoding("utf8");
    socket.on("data", (chunk) => {
      response += chunk;
    });
    socket.on("end", () => resolve(response));
    socket.on("error", reject);
  });
}

afterEach(() => {
  stopAuthServer();
  setRuntimeEnvironmentOverride(null);
  setupTestEnvironment();
});

describe("Auth Service", () => {
  test("returns valid token when present", async () => {
    withValidAuth();

    const token = await runEffect(getFromKeychainEffect("token"));
    expect(token).toBe("test-token-123");
  });

  test("returns null for expired token", async () => {
    withExpiredAuth();

    const token = await runEffect(getFromKeychainEffect("token"));
    expect(token).toBeNull();
  });

  test("returns null when no token exists", async () => {
    withNoAuth();

    const token = await runEffect(getFromKeychainEffect("token"));
    expect(token).toBeNull();
  });

  test("opens production OAuth authorize URL by default", async () => {
    withNoAuth();
    process.env.OAUTH_AUTH_URL = "";
    process.env.OAUTH_TOKEN_URL = "";
    process.env.GODADDY_OAUTH_CLIENT_ID = "";

    const login = runEffect(authLoginEffect());

    await vi.waitFor(() => {
      expect(mockOpen).toHaveBeenCalledWith(expect.any(String));
    });

    const openedUrl = new URL(String(mockOpen.mock.calls.at(-1)?.[0]));
    expect(openedUrl.origin).toBe("https://api.godaddy.com");
    expect(openedUrl.pathname).toBe("/v2/oauth2/authorize");
    expect(openedUrl.searchParams.get("client_id")).toBe(
      "39489dee-4103-4284-9aab-9f2452142bce",
    );

    const callbackUrl = new URL(openedUrl.searchParams.get("redirect_uri")!);
    callbackUrl.searchParams.set("code", "test-auth-code");
    callbackUrl.searchParams.set("state", openedUrl.searchParams.get("state")!);
    await rawHttpGet(callbackUrl);

    const result = await login;
    expect(result.success).toBe(true);
    expect(mockKeytar.setPassword).toHaveBeenCalledWith(
      "godaddy-cli",
      expect.stringMatching(/^token:v3:prod:/),
      expect.stringContaining('"accessToken":"test-token-123"'),
    );
  });
});
