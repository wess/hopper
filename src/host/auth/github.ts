// GitHub account connection. A stored personal access token raises the search
// rate limit, unlocks private-repo search (../registry/github.ts), and doubles
// as the GHCR pull password (../docker/credentials.ts#resolveAuth). The token
// lives only in the OS keychain; callers see the login, never the secret.

import type { GithubStatus } from "../../shared/types.ts";
import { clearGithub, getGithub, setGithub } from "./store.ts";

type GithubUser = { login?: string };

// Validate a token by fetching the authenticated user, then store it with the
// resolved login so status is offline-cheap.
export const githubConnect = async (
  token: string,
): Promise<{ ok: boolean; login?: string; error?: string }> => {
  const trimmed = token.trim();
  if (!trimmed) return { ok: false, error: "Token required" };

  try {
    const res = await fetch("https://api.github.com/user", {
      headers: {
        Authorization: `Bearer ${trimmed}`,
        Accept: "application/vnd.github+json",
        "User-Agent": "Hopper",
      },
    });
    if (res.status === 401) return { ok: false, error: "Invalid or expired token" };
    if (!res.ok) return { ok: false, error: `GitHub returned ${res.status}` };
    const user = (await res.json()) as GithubUser;
    await setGithub({ login: user.login, token: trimmed });
    return { ok: true, login: user.login };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : "Connection failed" };
  }
};

export const githubDisconnect = (): Promise<void> => clearGithub();

export const githubStatus = async (): Promise<GithubStatus> => {
  const cred = await getGithub();
  return cred ? { connected: true, login: cred.login } : { connected: false };
};

// The raw token for outbound GitHub/GHCR requests. Host-only — never sent to
// the webview.
export const githubToken = async (): Promise<string | undefined> => (await getGithub())?.token;
