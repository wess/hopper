// In-app registry sign-in. Validates credentials against the Docker daemon's
// /auth endpoint (exactly what `docker login` does), then stores them in the OS
// keychain so push/pull pick them up via ../docker/credentials.ts#resolveAuth —
// no `docker login` CLI required.

import type { RegistryConnection, RegistryLoginInput } from "../../shared/types.ts";
import { DockerError, json } from "../docker/client.ts";
import { canonicalServer, DOCKER_HUB_SERVER, listRegistries } from "../docker/credentials.ts";
import { dropAuth, loadAuths, putAuth } from "./store.ts";

type AuthReply = { Status?: string; IdentityToken?: string };

// A friendly label for a server address. Hub's canonical key is opaque, so
// special-case it; otherwise the bare host reads fine.
const labelFor = (server: string): string => (server === DOCKER_HUB_SERVER ? "Docker Hub" : server);

// Validate against the daemon and persist on success. The daemon may hand back
// an identity token (preferred — no stored password); otherwise we keep the
// username/password pair the way docker config would.
export const registryLogin = async (
  input: RegistryLoginInput,
): Promise<{ ok: boolean; error?: string }> => {
  const server = canonicalServer(input.server);
  const username = input.username.trim();
  if (!username || !input.password) return { ok: false, error: "Username and password required" };

  try {
    const reply = await json<AuthReply>("/auth", {
      method: "POST",
      body: { username, password: input.password, serveraddress: server },
    });
    const token = reply.IdentityToken?.trim();
    if (token) await putAuth(server, { username, secret: token, kind: "token" });
    else await putAuth(server, { username, secret: input.password, kind: "password" });
    return { ok: true };
  } catch (e) {
    // The daemon returns 401/403 with a message for bad creds; surface it.
    if (e instanceof DockerError) {
      return { ok: false, error: e.status === 401 ? "Invalid credentials" : e.message };
    }
    return { ok: false, error: e instanceof Error ? e.message : "Login failed" };
  }
};

export const registryLogout = async (server: string): Promise<void> => {
  await dropAuth(canonicalServer(server));
};

// Connections shown in the Accounts panel: Hopper-owned keychain credentials
// first (we can log those out), then any registries from the user's docker
// config that we didn't already cover (read-only, surfaced for context).
export const registryConnections = async (): Promise<RegistryConnection[]> => {
  const stored = await loadAuths();
  const out: RegistryConnection[] = Object.entries(stored).map(([server, cred]) => ({
    server,
    label: labelFor(server),
    username: cred.username,
    source: "hopper" as const,
  }));

  const owned = new Set(Object.keys(stored).map((s) => canonicalServer(s)));
  for (const server of await listRegistries()) {
    if (owned.has(canonicalServer(server))) continue;
    out.push({ server, label: labelFor(canonicalServer(server)), source: "docker" });
  }
  return out;
};
