const CHARS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

const randomString = (length: number): string => {
  const bytes = crypto.getRandomValues(new Uint8Array(length));
  let out = "";
  for (let i = 0; i < length; i++) out += CHARS[bytes[i]! % CHARS.length];
  return out;
};

const base64UrlEncode = (bytes: Uint8Array): string => {
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
};

export type PkcePair = {
  readonly verifier: string;
  readonly challenge: string;
  readonly method: "S256";
};

export const createPkce = async (): Promise<PkcePair> => {
  const verifier = randomString(64);
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier));
  const challenge = base64UrlEncode(new Uint8Array(digest));
  return { verifier, challenge, method: "S256" };
};

export const randomState = (length = 32): string => randomString(length);
