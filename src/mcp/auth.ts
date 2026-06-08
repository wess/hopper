// Bearer-token auth for the HTTP MCP transport.
//
// The stdio MCP trusts its parent process. Over HTTP it's reachable on the LAN,
// so every request must carry `Authorization: Bearer <token>`. The token comes
// from HOPPER_MCP_TOKEN. With no token configured the HTTP transport refuses to
// start (fail closed) rather than expose an unauthenticated exec surface.
//
// Env:
//   HOPPER_MCP_TOKEN   Shared secret tandem (and any other client) sends as a
//                      bearer token. REQUIRED to start the HTTP transport.

const BEARER = /^Bearer\s+(.+)$/i;

// Constant-time-ish comparison so a wrong token can't be probed byte-by-byte
// via response timing. Length leak is acceptable here.
const timingSafeEqual = (a: string, b: string): boolean => {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
};

export const readToken = (
  env: Record<string, string | undefined> = process.env,
): string | undefined => env.HOPPER_MCP_TOKEN?.trim() || undefined;

// True when the request's Authorization header matches the configured token.
export const isAuthorized = (header: string | null, token: string): boolean => {
  if (!header) return false;
  const m = header.match(BEARER);
  if (!m?.[1]) return false;
  return timingSafeEqual(m[1].trim(), token);
};
