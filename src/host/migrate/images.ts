// Image migration: stream each image from source to dest (save → load). The
// source GET body is piped straight into the dest load; if the dest call fails
// we cancel the source stream so its connection is released promptly rather
// than lingering until GC.

import type { Endpoint } from "../docker/endpoint.ts";
import { type Emit, step } from "./helpers.ts";
import { drainNdjson, reqOn } from "./transport.ts";

export const imageExists = async (ep: Endpoint, ref: string): Promise<boolean> => {
  try {
    // Tag/id colon stays unencoded in the path (matches docker/images.ts).
    await reqOn(ep, `/images/${ref}/json`);
    return true;
  } catch {
    return false;
  }
};

// Copy one image (by id or tag) from source to dest. Throws on any failure,
// including an in-band ndjson error frame from /images/load (see drainNdjson).
const transferImage = async (source: Endpoint, dest: Endpoint, ref: string): Promise<void> => {
  const saved = await reqOn(source, "/images/get", { query: { names: ref } });
  try {
    await drainNdjson(dest, "/images/load", {
      method: "POST",
      raw: true,
      body: saved.body ?? undefined,
      headers: { "Content-Type": "application/x-tar" },
    });
  } catch (e) {
    await saved.body?.cancel().catch(() => {});
    throw e;
  }
};

// Stream an image to dest only if it isn't already there.
export const ensureImage = async (source: Endpoint, dest: Endpoint, ref: string): Promise<void> => {
  if (await imageExists(dest, ref)) return;
  await transferImage(source, dest, ref);
};

export const migrateImages = async (
  source: Endpoint,
  dest: Endpoint,
  ids: readonly string[],
  emit: Emit,
): Promise<void> => {
  let done = 0;
  for (const id of ids) {
    step(
      emit,
      "images",
      id.slice(0, 19),
      done,
      ids.length,
      `Transferring image ${id.slice(0, 19)}`,
    );
    try {
      await transferImage(source, dest, id);
    } catch (e) {
      step(emit, "images", id.slice(0, 19), done, ids.length, "Image failed", {
        error: String(e),
      });
    }
    done++;
  }
};
