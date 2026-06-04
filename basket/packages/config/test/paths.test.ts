import { describe, expect, test } from "bun:test";
import { paths } from "../index.ts";

describe("paths", () => {
  test("returns the four roles", () => {
    const p = paths({ name: "TestApp" });
    expect(typeof p.data).toBe("string");
    expect(typeof p.config).toBe("string");
    expect(typeof p.cache).toBe("string");
    expect(typeof p.logs).toBe("string");
  });

  test("paths include the app name", () => {
    const p = paths({ name: "BasketTestApp" });
    expect(p.data.includes("BasketTestApp")).toBe(true);
    expect(p.config.includes("BasketTestApp")).toBe(true);
  });

  test("returns frozen object", () => {
    const p = paths({ name: "X" });
    expect(Object.isFrozen(p)).toBe(true);
  });
});
