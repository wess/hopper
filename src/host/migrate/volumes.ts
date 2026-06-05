// Volume migration: copy each named volume's data with the archive API.
//
// A helper container on each side mounts the volume at /v; we GET a tar of /v
// from the source and PUT it back into /v on the dest. The helpers are CREATED
// but never STARTED — the daemon exposes a container's volume mounts to the
// archive API without running it (verified against Docker Desktop), so the
// helper image's entrypoint is irrelevant and any image works. We reuse a small
// image already on the source (transferring it to the dest once) so we NEVER
// pull onto the source — the migration only ever reads it.

import type { Endpoint } from "../docker/endpoint.ts";
import { type Emit, step } from "./helpers.ts";
import { ensureImage } from "./images.ts";
import { jsonOn, reqOn } from "./transport.ts";

type RawImage = { Id: string; RepoTags?: string[] | null; Size?: number };

// Pick the smallest image already present on the source (preferring busybox) to
// use as a throwaway mount helper. Never pulls.
const pickHelperImage = async (source: Endpoint): Promise<string> => {
  const imgs = await jsonOn<RawImage[]>(source, "/images/json", {});
  const tagged = imgs
    .map((i) => ({
      ref: (i.RepoTags ?? []).find((t) => t && t !== "<none>:<none>"),
      size: i.Size ?? Number.MAX_SAFE_INTEGER,
    }))
    .filter((x): x is { ref: string; size: number } => Boolean(x.ref));
  const busybox = tagged.find((x) => x.ref.includes("busybox"));
  if (busybox) return busybox.ref;
  tagged.sort((a, b) => a.size - b.size);
  if (tagged[0]) return tagged[0].ref;
  if (imgs[0]?.Id) return imgs[0].Id;
  throw new Error("source engine has no image to use as a copy helper");
};

const volumeExistsOnDest = async (dest: Endpoint, name: string): Promise<boolean> => {
  try {
    await reqOn(dest, `/volumes/${name}`);
    return true;
  } catch {
    return false;
  }
};

const copyVolume = async (
  source: Endpoint,
  dest: Endpoint,
  name: string,
  helperImage: string,
): Promise<void> => {
  await jsonOn(dest, "/volumes/create", { method: "POST", body: { Name: name } });

  // Created, not started — the archive API still sees the volume mount.
  const make = async (ep: Endpoint, readOnly: boolean): Promise<string> => {
    const created = await jsonOn<{ Id: string }>(ep, "/containers/create", {
      method: "POST",
      body: { Image: helperImage, HostConfig: { Binds: [`${name}:/v${readOnly ? ":ro" : ""}`] } },
    });
    return created.Id;
  };
  const rm = (ep: Endpoint, id: string): Promise<unknown> =>
    reqOn(ep, `/containers/${id}`, { method: "DELETE", query: { force: true, v: false } }).catch(
      () => {},
    );

  // Declare ids before the try so the finally always cleans up whatever was
  // created, even if a later create throws.
  let cs: string | undefined;
  let cd: string | undefined;
  try {
    cs = await make(source, true);
    cd = await make(dest, false);
    // GET tar is rooted at "v/"; extracting at "/" on the dest lands it in /v.
    const tar = await reqOn(source, `/containers/${cs}/archive`, { query: { path: "/v" } });
    try {
      await reqOn(dest, `/containers/${cd}/archive`, {
        method: "PUT",
        query: { path: "/" },
        raw: true,
        body: tar.body ?? undefined,
        headers: { "Content-Type": "application/x-tar" },
      });
    } catch (e) {
      await tar.body?.cancel().catch(() => {});
      throw e;
    }
  } finally {
    if (cs) await rm(source, cs);
    if (cd) await rm(dest, cd);
  }
};

export const migrateVolumes = async (
  source: Endpoint,
  dest: Endpoint,
  names: readonly string[],
  emit: Emit,
): Promise<void> => {
  if (names.length === 0) return;

  // Resolve the mount-helper image once (and make it available on the dest).
  // A failure here only skips the volume phase — images/containers still run.
  let helperImage: string;
  try {
    helperImage = await pickHelperImage(source);
    await ensureImage(source, dest, helperImage);
  } catch (e) {
    step(emit, "volumes", "", 0, names.length, "Volume copy unavailable (no helper image)", {
      error: String(e),
    });
    return;
  }

  let done = 0;
  for (const name of names) {
    step(emit, "volumes", name, done, names.length, `Copying volume ${name}`);
    try {
      if (await volumeExistsOnDest(dest, name)) {
        // Don't silently merge into / overwrite an existing dest volume.
        step(emit, "volumes", name, done, names.length, `Volume ${name} already present`, {
          warning: `${name} already exists on the destination — left as-is, not overwritten`,
        });
      } else {
        await copyVolume(source, dest, name, helperImage);
      }
    } catch (e) {
      step(emit, "volumes", name, done, names.length, `Volume ${name} failed`, {
        error: String(e),
      });
    }
    done++;
  }
};
