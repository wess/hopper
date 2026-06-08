// Keychain-backed credential store for Hopper-owned logins. Unlike the docker
// config path in ../docker/credentials.ts (which Hopper only reads), this is
// what lets a user sign in to a registry or GitHub from inside the app with no
// CLI — the secrets live in the OS keychain via @basket/secrets.
//
// @basket/secrets keys must match [a-zA-Z0-9._-], so registry server addresses
// (which contain "/" and ":") can't be keys. We keep all registry credentials
// in one JSON document under a single fixed key, and the GitHub token under
// another.

import { deleteSecret, getSecret, setSecret } from "@basket/secrets";

const SERVICE = "io.wess.hopper";
const REGISTRY_KEY = "registry.auths";
const GITHUB_KEY = "github.token";

// One stored registry credential. `kind: "token"` means the secret is a
// daemon identity token (the `/auth` response gave one); otherwise it's the
// account password / access token paired with `username`.
export type StoredCred = {
  readonly username?: string;
  readonly secret: string;
  readonly kind: "password" | "token";
};

type AuthDoc = Record<string, StoredCred>;

const readDoc = async (): Promise<AuthDoc> => {
  const raw = await getSecret(SERVICE, REGISTRY_KEY);
  if (!raw) return {};
  try {
    return JSON.parse(raw) as AuthDoc;
  } catch {
    return {};
  }
};

const writeDoc = async (doc: AuthDoc): Promise<void> => {
  if (Object.keys(doc).length === 0) {
    await deleteSecret(SERVICE, REGISTRY_KEY);
    return;
  }
  await setSecret(SERVICE, REGISTRY_KEY, JSON.stringify(doc));
};

export const loadAuths = (): Promise<AuthDoc> => readDoc();

export const getAuth = async (server: string): Promise<StoredCred | undefined> =>
  (await readDoc())[server];

export const putAuth = async (server: string, cred: StoredCred): Promise<void> => {
  const doc = await readDoc();
  doc[server] = cred;
  await writeDoc(doc);
};

export const dropAuth = async (server: string): Promise<void> => {
  const doc = await readDoc();
  if (!(server in doc)) return;
  delete doc[server];
  await writeDoc(doc);
};

// --- GitHub token ---------------------------------------------------------
// Stored as single-line JSON so status can report the account without a network
// round-trip. (Must stay newline-free: the macOS keychain returns values
// containing a newline as hex, corrupting them on read.)

export type GithubCred = { readonly login?: string; readonly token: string };

export const getGithub = async (): Promise<GithubCred | undefined> => {
  const raw = await getSecret(SERVICE, GITHUB_KEY);
  if (!raw) return undefined;
  try {
    const parsed = JSON.parse(raw) as Partial<GithubCred>;
    if (typeof parsed.token === "string") return { login: parsed.login, token: parsed.token };
  } catch {
    // Legacy/plain value — treat the whole string as the token.
  }
  return { token: raw };
};

export const setGithub = async (cred: GithubCred): Promise<void> => {
  await setSecret(SERVICE, GITHUB_KEY, JSON.stringify({ login: cred.login, token: cred.token }));
};

export const clearGithub = async (): Promise<void> => {
  await deleteSecret(SERVICE, GITHUB_KEY);
};
