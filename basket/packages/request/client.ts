import type { Client, ClientOptions, RequestInit, Response } from "./types.ts";

const expBackoff = (attempt: number): number => Math.min(30_000, 100 * 2 ** attempt);

const defaultRetryOn = (status: number): boolean => status >= 500 || status === 429;

const buildUrl = (url: string, base?: string, query?: RequestInit["query"]): string => {
  const u = base ? new URL(url, base) : new URL(url);
  if (query) {
    for (const [k, v] of Object.entries(query)) {
      if (v !== undefined && v !== null) u.searchParams.set(k, String(v));
    }
  }
  return u.toString();
};

const isJson = (headers: Headers): boolean => (headers.get("content-type") ?? "").includes("application/json");

const serializeBody = (body: unknown, headers: Record<string, string>): BodyInit | undefined => {
  if (body === undefined || body === null) return undefined;
  if (typeof body === "string" || body instanceof FormData || body instanceof Blob || body instanceof ArrayBuffer) {
    return body as BodyInit;
  }
  headers["content-type"] = headers["content-type"] ?? "application/json";
  return JSON.stringify(body);
};

const mergeHeaders = (...sources: (Record<string, string> | undefined)[]): Record<string, string> => {
  const out: Record<string, string> = {};
  for (const src of sources) {
    if (!src) continue;
    for (const [k, v] of Object.entries(src)) out[k.toLowerCase()] = v;
  }
  return out;
};

const REQUEST_ERROR_TAG = "__basketRequestError__";

export type RequestError = Error & {
  readonly status: number;
  readonly response: globalThis.Response;
  readonly data: unknown;
};

export const isRequestError = (e: unknown): e is RequestError =>
  e instanceof Error && (e as unknown as Record<string, unknown>)[REQUEST_ERROR_TAG] === true;

const requestError = (status: number, response: globalThis.Response, data: unknown, message: string): RequestError => {
  const err = new Error(message);
  Object.assign(err, { [REQUEST_ERROR_TAG]: true, status, response, data });
  return err as RequestError;
};

export const createClient = (options: ClientOptions = {}): Client => {
  const reqInterceptors = options.requestInterceptors ?? [];
  const resInterceptors = options.responseInterceptors ?? [];
  const retries = options.retries ?? 0;
  const retryDelay = options.retryDelay ?? expBackoff;
  const retryOn = options.retryOn ?? defaultRetryOn;

  const execute = async <T>(init: RequestInit): Promise<Response<T>> => {
    let current: RequestInit = init;
    for (const i of reqInterceptors) current = await i(current);

    const headers = mergeHeaders(options.headers, current.headers);
    const body = serializeBody(current.body, headers);
    const url = buildUrl(current.url, options.baseURL, current.query);
    const method = current.method ?? "GET";

    const controller = new AbortController();
    const signal = current.signal;
    if (signal) signal.addEventListener("abort", () => controller.abort(), { once: true });
    const timeout = current.timeout ?? options.timeout;
    const timer = timeout ? setTimeout(() => controller.abort(), timeout) : undefined;

    try {
      const totalAttempts = (current.retries ?? retries) + 1;
      let lastError: unknown;
      for (let attempt = 0; attempt < totalAttempts; attempt++) {
        try {
          const raw = await fetch(url, { method, headers, body, signal: controller.signal });
          if (!raw.ok && retryOn(raw.status) && attempt < totalAttempts - 1) {
            await new Promise((r) => setTimeout(r, (current.retryDelay ?? retryDelay)(attempt)));
            continue;
          }

          const data = isJson(raw.headers) ? ((await raw.json()) as T) : ((await raw.text()) as T);

          if (!raw.ok) {
            throw requestError(raw.status, raw, data, `Request failed with status ${raw.status}`);
          }

          let response: Response<T> = { status: raw.status, headers: raw.headers, data, raw };
          for (const i of resInterceptors) response = (await i(response)) as Response<T>;
          return response;
        } catch (e) {
          lastError = e;
          if (isRequestError(e)) throw e;
          if (attempt < totalAttempts - 1 && retryOn(0, e)) {
            await new Promise((r) => setTimeout(r, (current.retryDelay ?? retryDelay)(attempt)));
            continue;
          }
          throw e;
        }
      }
      throw lastError ?? new Error("request failed");
    } finally {
      if (timer) clearTimeout(timer);
    }
  };

  const withBody = <T>(
    url: string,
    body: unknown,
    method: "POST" | "PUT" | "PATCH",
    init?: Omit<RequestInit, "url" | "method" | "body">,
  ): Promise<Response<T>> => execute<T>({ ...init, url, method, body });

  return {
    request: execute,
    get: (url, init) => execute({ ...init, url, method: "GET" }),
    post: (url, body, init) => withBody(url, body, "POST", init),
    put: (url, body, init) => withBody(url, body, "PUT", init),
    patch: (url, body, init) => withBody(url, body, "PATCH", init),
    delete: (url, init) => execute({ ...init, url, method: "DELETE" }),
  };
};
