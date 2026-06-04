import { openBrowser } from "./browser.ts";
import { createPkce, randomState } from "./pkce.ts";

export type OAuthOptions = {
  readonly authUrl: string;
  readonly tokenUrl: string;
  readonly clientId: string;
  readonly clientSecret?: string;
  readonly scopes: readonly string[];
  readonly redirectPort?: number;
  readonly redirectPath?: string;
  readonly responseType?: "code";
  readonly extraAuthParams?: Record<string, string>;
};

export type TokenResponse = {
  readonly accessToken: string;
  readonly refreshToken?: string;
  readonly expiresIn?: number;
  readonly tokenType?: string;
  readonly scope?: string;
  readonly raw: Record<string, unknown>;
};

export type OAuthClient = {
  readonly start: () => Promise<TokenResponse>;
  readonly refresh: (refreshToken: string) => Promise<TokenResponse>;
};

const SUCCESS_HTML = `<!doctype html><html><head><title>Signed in</title>
<style>
body{font:14px -apple-system,Segoe UI,sans-serif;display:grid;place-items:center;height:100vh;margin:0;background:#0f0f10;color:#f0f0f0}
div{text-align:center;padding:2rem;border-radius:12px}
h1{margin:0 0 .5rem;font-weight:600}
p{color:#999;margin:0}
</style></head><body><div><h1>Signed in</h1><p>You can close this window.</p></div></body></html>`;

const ERROR_HTML = (msg: string): string =>
  `<!doctype html><html><body style="font:14px sans-serif;padding:2rem"><h1>Authentication failed</h1><pre>${msg}</pre></body></html>`;

const parseToken = (raw: Record<string, unknown>): TokenResponse => ({
  accessToken: String(raw.access_token ?? ""),
  refreshToken: raw.refresh_token as string | undefined,
  expiresIn: raw.expires_in as number | undefined,
  tokenType: raw.token_type as string | undefined,
  scope: raw.scope as string | undefined,
  raw,
});

const exchange = async (opts: OAuthOptions, params: Record<string, string>): Promise<TokenResponse> => {
  const headers: Record<string, string> = {
    "content-type": "application/x-www-form-urlencoded",
    accept: "application/json",
  };
  const body = new URLSearchParams(params).toString();
  const res = await fetch(opts.tokenUrl, { method: "POST", headers, body });
  if (!res.ok) {
    throw new Error(`Token exchange failed: ${res.status} ${await res.text()}`);
  }
  const text = await res.text();
  let data: Record<string, unknown>;
  try {
    data = JSON.parse(text);
  } catch {
    data = Object.fromEntries(new URLSearchParams(text));
  }
  if (data.error) throw new Error(`OAuth error: ${data.error} ${data.error_description ?? ""}`);
  return parseToken(data);
};

export const createOAuthClient = (opts: OAuthOptions): OAuthClient => {
  const redirectPath = opts.redirectPath ?? "/callback";
  const port = opts.redirectPort ?? 53682;
  const redirectUri = `http://127.0.0.1:${port}${redirectPath}`;

  const start = async (): Promise<TokenResponse> => {
    const pkce = await createPkce();
    const state = randomState();

    const url = new URL(opts.authUrl);
    url.searchParams.set("response_type", opts.responseType ?? "code");
    url.searchParams.set("client_id", opts.clientId);
    url.searchParams.set("redirect_uri", redirectUri);
    url.searchParams.set("scope", opts.scopes.join(" "));
    url.searchParams.set("state", state);
    url.searchParams.set("code_challenge", pkce.challenge);
    url.searchParams.set("code_challenge_method", pkce.method);
    for (const [k, v] of Object.entries(opts.extraAuthParams ?? {})) url.searchParams.set(k, v);

    return new Promise<TokenResponse>((resolve, reject) => {
      const server = Bun.serve({
        port,
        hostname: "127.0.0.1",
        async fetch(req) {
          const reqUrl = new URL(req.url);
          if (reqUrl.pathname !== redirectPath) return new Response("not found", { status: 404 });

          const error = reqUrl.searchParams.get("error");
          if (error) {
            queueMicrotask(() => {
              server.stop();
              reject(new Error(`OAuth error: ${error} ${reqUrl.searchParams.get("error_description") ?? ""}`));
            });
            return new Response(ERROR_HTML(error), { status: 400, headers: { "content-type": "text/html" } });
          }

          const code = reqUrl.searchParams.get("code");
          const returnedState = reqUrl.searchParams.get("state");
          if (!code || returnedState !== state) {
            queueMicrotask(() => {
              server.stop();
              reject(new Error("Missing code or state mismatch"));
            });
            return new Response(ERROR_HTML("state mismatch"), {
              status: 400,
              headers: { "content-type": "text/html" },
            });
          }

          try {
            const token = await exchange(opts, {
              grant_type: "authorization_code",
              code,
              redirect_uri: redirectUri,
              client_id: opts.clientId,
              code_verifier: pkce.verifier,
              ...(opts.clientSecret ? { client_secret: opts.clientSecret } : {}),
            });
            queueMicrotask(() => {
              server.stop();
              resolve(token);
            });
            return new Response(SUCCESS_HTML, { headers: { "content-type": "text/html" } });
          } catch (e) {
            queueMicrotask(() => {
              server.stop();
              reject(e);
            });
            return new Response(ERROR_HTML(String(e)), { status: 500, headers: { "content-type": "text/html" } });
          }
        },
      });

      openBrowser(url.toString()).catch(reject);
    });
  };

  const refresh = (refreshToken: string): Promise<TokenResponse> =>
    exchange(opts, {
      grant_type: "refresh_token",
      refresh_token: refreshToken,
      client_id: opts.clientId,
      ...(opts.clientSecret ? { client_secret: opts.clientSecret } : {}),
    });

  return { start, refresh };
};
