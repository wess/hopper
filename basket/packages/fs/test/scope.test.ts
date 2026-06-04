import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { memoryStore } from "@basket/store";
import { createRecents, createScope } from "../index.ts";

let root: string;

beforeEach(async () => {
  root = await mkdtemp(join(tmpdir(), "basket-fs-"));
});

afterEach(async () => {
  await rm(root, { recursive: true, force: true });
});

describe("createScope", () => {
  test("write + read round-trip", async () => {
    const scope = createScope(root);
    await scope.writeJson("settings.json", { theme: "dark" });
    expect(await scope.readJson<{ theme: string }>("settings.json")).toEqual({ theme: "dark" });
  });

  test("rejects escape via ..", () => {
    const scope = createScope(root);
    expect(() => scope.resolve("../../../etc/passwd")).toThrow(/escapes scope/);
  });

  test("ensureDir creates nested directories", async () => {
    const scope = createScope(root);
    await scope.ensureDir("a/b/c");
    expect(await scope.exists("a/b/c")).toBe(true);
  });

  test("list returns entries", async () => {
    const scope = createScope(root);
    await scope.write("a.txt", "1");
    await scope.write("b.txt", "2");
    const entries = await scope.list();
    expect(entries.map((e) => e.name).sort()).toEqual(["a.txt", "b.txt"]);
  });
});

describe("createRecents", () => {
  test("add/list/remove with LRU semantics", () => {
    const recents = createRecents({ store: memoryStore(), limit: 3 });
    recents.add("/a");
    recents.add("/b");
    recents.add("/c");
    recents.add("/a");
    expect(recents.list()).toEqual(["/a", "/c", "/b"]);
    recents.add("/d");
    expect(recents.list()).toEqual(["/d", "/a", "/c"]);
    recents.remove("/a");
    expect(recents.list()).toEqual(["/d", "/c"]);
  });
});
