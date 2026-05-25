import { type } from "arktype";

/**
 * Returns true when `value` is a syntactically valid `http(s)` URL whose host
 * is expected to be resolvable on the public internet.
 *
 * The following host classes are rejected:
 *  - `localhost` and any `*.localhost` / `*.local` hostnames
 *  - IPv4 loopback `127.0.0.0/8` and the unspecified address `0.0.0.0`
 *  - IPv6 loopback `::1` and unspecified `::`
 *  - IPv4 link-local `169.254.0.0/16`
 *  - IPv6 link-local `fe80::/10`
 *  - RFC1918 private IPv4 ranges: `10.0.0.0/8`, `172.16.0.0/12`,
 *    `192.168.0.0/16`
 *
 * This is enforced client-side so users get an actionable error immediately
 * rather than an opaque 403 from the upstream WAF.
 */
export function isPublicRoutableUrl(value: string): boolean {
  if (typeof value !== "string" || value.trim() === "") {
    return false;
  }

  let parsed: URL;
  try {
    parsed = new URL(value);
  } catch {
    return false;
  }

  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    return false;
  }

  const rawHost = parsed.hostname.toLowerCase();

  if (rawHost === "") {
    return false;
  }

  // WHATWG URL returns IPv6 hostnames wrapped in square brackets.
  const host =
    rawHost.startsWith("[") && rawHost.endsWith("]")
      ? rawHost.slice(1, -1)
      : rawHost;

  if (host === "localhost") {
    return false;
  }

  if (host.endsWith(".localhost") || host.endsWith(".local")) {
    return false;
  }

  if (isIPv4(host)) {
    return isPublicIPv4(host);
  }

  if (isIPv6(host)) {
    return isPublicIPv6(host);
  }

  return true;
}

function isIPv4(host: string): boolean {
  return /^(\d{1,3}\.){3}\d{1,3}$/.test(host);
}

function parseIPv4Octets(host: string): number[] | null {
  const parts = host.split(".").map((p) => Number(p));
  if (parts.length !== 4) return null;
  for (const p of parts) {
    if (!Number.isInteger(p) || p < 0 || p > 255) return null;
  }
  return parts;
}

function isPublicIPv4(host: string): boolean {
  const octets = parseIPv4Octets(host);
  if (!octets) return false;
  const [a, b] = octets as [number, number, number, number];

  // Unspecified 0.0.0.0/8
  if (a === 0) return false;
  // Loopback 127.0.0.0/8
  if (a === 127) return false;
  // RFC1918: 10.0.0.0/8
  if (a === 10) return false;
  // RFC1918: 172.16.0.0/12
  if (a === 172 && b >= 16 && b <= 31) return false;
  // RFC1918: 192.168.0.0/16
  if (a === 192 && b === 168) return false;
  // Link-local 169.254.0.0/16
  if (a === 169 && b === 254) return false;

  return true;
}

function isIPv6(host: string): boolean {
  // WHATWG URL parses IPv6 hostnames wrapped in [] and returns them
  // unwrapped in .hostname, so detect by the presence of a ":".
  return host.includes(":");
}

function isPublicIPv6(host: string): boolean {
  const normalized = host.toLowerCase();

  if (normalized === "::" || normalized === "::1") return false;
  if (normalized.startsWith("fe80:") || normalized.startsWith("fe80::")) {
    return false;
  }

  return true;
}

/**
 * Arktype narrowing that accepts only publicly-routable `http(s)` URLs.
 *
 * Use in schemas in place of `type.keywords.string.url.root` when the URL
 * will be stored server-side and needs to be reachable from the public
 * internet (for example: app homepage URLs and OAuth redirect URIs).
 */
export const publicHttpUrl = type("string").narrow((value, ctx) => {
  if (isPublicRoutableUrl(value)) {
    return true;
  }
  return ctx.mustBe(
    "a publicly-resolvable http(s) URL (localhost, loopback, and private IPs are not allowed)",
  );
});
