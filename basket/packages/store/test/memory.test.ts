import { describe, expect, test } from "bun:test";
import { memoryStore } from "../index.ts";

describe("memoryStore", () => {
  test("get/set/delete round-trip", () => {
    const s = memoryStore();
    s.set("a", 1);
    expect(s.get<number>("a")).toBe(1);
    s.delete("a");
    expect(s.has("a")).toBe(false);
  });

  test("defaults applied at construction", () => {
    const s = memoryStore({ theme: "dark" });
    expect(s.get<string>("theme")).toBe("dark");
  });

  test("clear restores defaults", () => {
    const s = memoryStore({ theme: "dark" });
    s.set("theme", "light");
    s.clear();
    expect(s.get<string>("theme")).toBe("dark");
  });

  test("all returns frozen snapshot", () => {
    const s = memoryStore({ a: 1 });
    const snap = s.all();
    expect(Object.isFrozen(snap)).toBe(true);
    expect(snap.a).toBe(1);
  });
});
