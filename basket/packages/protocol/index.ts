import { on } from "butter";

export type DeepLink = {
  readonly scheme: string;
  readonly host: string;
  readonly path: string;
  readonly params: Readonly<Record<string, string>>;
  readonly url: string;
};

export const parseDeepLink = (raw: string): DeepLink => {
  const url = new URL(raw);
  const params: Record<string, string> = {};
  for (const [k, v] of url.searchParams) params[k] = v;
  return {
    scheme: url.protocol.replace(/:$/, ""),
    host: url.hostname,
    path: url.pathname,
    params,
    url: raw,
  };
};

export type ProtocolHandler = (link: DeepLink) => void | Promise<void>;

const handlers = new Map<string, ProtocolHandler>();
let listening = false;

const dispatch = (raw: string): void => {
  let link: DeepLink;
  try {
    link = parseDeepLink(raw);
  } catch {
    return;
  }
  const handler = handlers.get(link.scheme) ?? handlers.get("*");
  if (handler) void handler(link);
};

const ensureListener = (): void => {
  if (listening) return;
  listening = true;
  on("app:openurl", (data) => {
    const url = typeof data === "string" ? data : (data as { url?: string })?.url;
    if (url) dispatch(url);
  });
};

export const onProtocol = (scheme: string, handler: ProtocolHandler): (() => void) => {
  ensureListener();
  handlers.set(scheme, handler);
  return () => handlers.delete(scheme);
};

export const onAnyProtocol = (handler: ProtocolHandler): (() => void) => onProtocol("*", handler);
