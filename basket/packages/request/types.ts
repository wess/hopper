export type Method = "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS";

export type Query = Record<string, string | number | boolean | null | undefined>;

export type RequestInit = {
  readonly url: string;
  readonly method?: Method;
  readonly headers?: Record<string, string>;
  readonly query?: Query;
  readonly body?: unknown;
  readonly signal?: AbortSignal;
  readonly timeout?: number;
  readonly retries?: number;
  readonly retryDelay?: (attempt: number) => number;
};

export type Response<T = unknown> = {
  readonly status: number;
  readonly headers: Headers;
  readonly data: T;
  readonly raw: globalThis.Response;
};

export type RequestInterceptor = (init: RequestInit) => RequestInit | Promise<RequestInit>;
export type ResponseInterceptor = <T>(res: Response<T>) => Response<T> | Promise<Response<T>>;

export type ClientOptions = {
  readonly baseURL?: string;
  readonly headers?: Record<string, string>;
  readonly timeout?: number;
  readonly retries?: number;
  readonly retryDelay?: (attempt: number) => number;
  readonly retryOn?: (status: number, error?: unknown) => boolean;
  readonly requestInterceptors?: readonly RequestInterceptor[];
  readonly responseInterceptors?: readonly ResponseInterceptor[];
};

export type Client = {
  readonly request: <T = unknown>(init: RequestInit) => Promise<Response<T>>;
  readonly get: <T = unknown>(url: string, init?: Omit<RequestInit, "url" | "method">) => Promise<Response<T>>;
  readonly post: <T = unknown>(
    url: string,
    body?: unknown,
    init?: Omit<RequestInit, "url" | "method" | "body">,
  ) => Promise<Response<T>>;
  readonly put: <T = unknown>(
    url: string,
    body?: unknown,
    init?: Omit<RequestInit, "url" | "method" | "body">,
  ) => Promise<Response<T>>;
  readonly patch: <T = unknown>(
    url: string,
    body?: unknown,
    init?: Omit<RequestInit, "url" | "method" | "body">,
  ) => Promise<Response<T>>;
  readonly delete: <T = unknown>(url: string, init?: Omit<RequestInit, "url" | "method">) => Promise<Response<T>>;
};
