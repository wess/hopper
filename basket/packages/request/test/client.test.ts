import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { createClient, isRequestError, type RequestError } from "../index.ts";

let server: ReturnType<typeof Bun.serve>;
let baseURL: string;
let attempts = 0;

beforeAll(() => {
  server = Bun.serve({
    port: 0,
    async fetch(req) {
      const url = new URL(req.url);
      if (url.pathname === "/echo") {
        const body = req.headers.get("content-type")?.includes("json") ? await req.json() : await req.text();
        return Response.json({ method: req.method, body, q: Object.fromEntries(url.searchParams) });
      }
      if (url.pathname === "/flaky") {
        attempts++;
        if (attempts < 3) return new Response("nope", { status: 503 });
        return Response.json({ ok: true, attempts });
      }
      if (url.pathname === "/forbidden") return new Response("nope", { status: 403 });
      return new Response("not found", { status: 404 });
    },
  });
  baseURL = `http://localhost:${server.port}`;
});

afterAll(() => server.stop(true));

describe("@basket/request", () => {
  test("get + post round-trip", async () => {
    const api = createClient({ baseURL });
    const { data } = await api.post<{ method: string; body: { x: number } }>("/echo", { x: 1 });
    expect(data.method).toBe("POST");
    expect(data.body.x).toBe(1);
  });

  test("query params", async () => {
    const api = createClient({ baseURL });
    const { data } = await api.get<{ q: Record<string, string> }>("/echo", { query: { a: 1, b: "two" } });
    expect(data.q).toEqual({ a: "1", b: "two" });
  });

  test("retries on 5xx", async () => {
    attempts = 0;
    const api = createClient({ baseURL, retries: 5, retryDelay: () => 1 });
    const { data } = await api.get<{ attempts: number }>("/flaky");
    expect(data.attempts).toBe(3);
  });

  test("throws RequestError on non-retryable failure", async () => {
    const api = createClient({ baseURL });
    let err: unknown;
    try {
      await api.get("/forbidden");
    } catch (e) {
      err = e;
    }
    expect(isRequestError(err)).toBe(true);
    expect((err as RequestError).status).toBe(403);
  });

  test("interceptors mutate request and response", async () => {
    const api = createClient({
      baseURL,
      requestInterceptors: [(init) => ({ ...init, headers: { ...init.headers, "x-trace": "1" } })],
      responseInterceptors: [(res) => ({ ...res, data: { wrapped: res.data } as never })],
    });
    const { data } = await api.get<{ wrapped: unknown }>("/echo");
    expect("wrapped" in data).toBe(true);
  });
});
