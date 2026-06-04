import { describe, expect, test } from "bun:test";
import { createPkce, hashPassword, randomState, verifyPassword } from "../index.ts";

describe("@basket/auth", () => {
  test("hash + verify round-trip", async () => {
    const hash = await hashPassword("hunter2");
    expect(await verifyPassword("hunter2", hash)).toBe(true);
    expect(await verifyPassword("wrong", hash)).toBe(false);
  });

  test("pkce produces verifier + challenge", async () => {
    const pkce = await createPkce();
    expect(pkce.method).toBe("S256");
    expect(pkce.verifier.length).toBeGreaterThanOrEqual(43);
    expect(pkce.challenge.length).toBeGreaterThan(0);
    expect(pkce.challenge.includes("=")).toBe(false);
  });

  test("randomState varies", () => {
    expect(randomState()).not.toBe(randomState());
  });
});
