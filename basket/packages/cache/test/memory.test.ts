import { describe, expect, test } from "bun:test";
import { memoryCache } from "../index.ts";

describe("memoryCache", () => {
  test("set + get", async () => {
    const c = memoryCache();
    await c.set("k", 1);
    expect(await c.get<number>("k")).toBe(1);
  });

  test("ttl expires", async () => {
    const c = memoryCache();
    await c.set("k", 1, 5);
    await new Promise((r) => setTimeout(r, 15));
    expect(await c.get("k")).toBeUndefined();
  });

  test("aside runs loader once when fresh", async () => {
    const c = memoryCache({ defaultTtlMs: 1000 });
    let calls = 0;
    const load = () => {
      calls++;
      return Promise.resolve("v");
    };
    expect(await c.aside("k", load)).toBe("v");
    expect(await c.aside("k", load)).toBe("v");
    expect(calls).toBe(1);
  });

  test("delete + clear", async () => {
    const c = memoryCache();
    await c.set("a", 1);
    await c.set("b", 2);
    await c.delete("a");
    expect(await c.has("a")).toBe(false);
    await c.clear();
    expect(await c.has("b")).toBe(false);
  });
});
