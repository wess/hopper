import { describe, expect, test } from "bun:test";
import { defineConfig, env } from "../index.ts";

describe("defineConfig", () => {
  test("resolves env refs into values", () => {
    Bun.env.TEST_NAME = "hello";
    const c = defineConfig({ name: env("TEST_NAME") });
    expect(c.name).toBe("hello");
  });

  test("respects parse and default", () => {
    delete Bun.env.TEST_PORT;
    const c = defineConfig({ port: env("TEST_PORT", { parse: Number, default: "3000" }) });
    expect(c.port).toBe(3000);
  });

  test("freezes nested objects", () => {
    const c = defineConfig({ app: { name: "x" } });
    expect(Object.isFrozen(c)).toBe(true);
    expect(Object.isFrozen(c.app)).toBe(true);
  });

  test("throws when required var missing", () => {
    delete Bun.env.MISSING_VAR;
    expect(() => defineConfig({ x: env("MISSING_VAR") })).toThrow(/Missing required/);
  });
});
