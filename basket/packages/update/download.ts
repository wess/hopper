import { mkdir } from "node:fs/promises";
import { dirname } from "node:path";

export type DownloadOptions = {
  readonly url: string;
  readonly to: string;
  readonly sha256?: string;
  readonly onProgress?: (received: number, total?: number) => void;
};

const sha256Hex = async (bytes: Uint8Array): Promise<string> => {
  const buf = new ArrayBuffer(bytes.byteLength);
  new Uint8Array(buf).set(bytes);
  const digest = await crypto.subtle.digest("SHA-256", buf);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
};

export const download = async (options: DownloadOptions): Promise<string> => {
  const res = await fetch(options.url);
  if (!res.ok || !res.body) throw new Error(`Download failed: ${res.status}`);

  const total = Number(res.headers.get("content-length")) || undefined;
  const reader = res.body.getReader();
  const chunks: Uint8Array[] = [];
  let received = 0;

  while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    if (value) {
      chunks.push(value);
      received += value.byteLength;
      options.onProgress?.(received, total);
    }
  }

  const combined = new Uint8Array(received);
  let offset = 0;
  for (const c of chunks) {
    combined.set(c, offset);
    offset += c.byteLength;
  }

  if (options.sha256) {
    const actual = await sha256Hex(combined);
    if (actual.toLowerCase() !== options.sha256.toLowerCase()) {
      throw new Error(`SHA256 mismatch: expected ${options.sha256}, got ${actual}`);
    }
  }

  await mkdir(dirname(options.to), { recursive: true });
  await Bun.write(options.to, combined);
  return options.to;
};
