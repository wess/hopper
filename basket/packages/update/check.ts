import { isNewer } from "./version.ts";

export type Manifest = {
  readonly version: string;
  readonly url: string;
  readonly notes?: string;
  readonly sha256?: string;
};

export type UpdateCheck =
  | { readonly available: false; readonly current: string }
  | { readonly available: true; readonly current: string; readonly manifest: Manifest };

export type CheckOptions = {
  readonly url: string;
  readonly current: string;
  readonly headers?: Record<string, string>;
};

export const check = async (options: CheckOptions): Promise<UpdateCheck> => {
  const res = await fetch(options.url, { headers: options.headers });
  if (!res.ok) throw new Error(`Update manifest fetch failed: ${res.status}`);
  const manifest = (await res.json()) as Manifest;
  if (!manifest.version || !manifest.url) {
    throw new Error("Manifest must include { version, url }");
  }
  if (isNewer(manifest.version, options.current)) {
    return { available: true, current: options.current, manifest };
  }
  return { available: false, current: options.current };
};
