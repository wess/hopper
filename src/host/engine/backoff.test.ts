import { describe, expect, test } from "bun:test";
import { backoffDelay, shouldRetry } from "./backoff.ts";

describe("backoffDelay", () => {
  test("grows exponentially from the base", () => {
    expect(backoffDelay(1, { baseMs: 500, factor: 2 })).toBe(500);
    expect(backoffDelay(2, { baseMs: 500, factor: 2 })).toBe(1000);
    expect(backoffDelay(3, { baseMs: 500, factor: 2 })).toBe(2000);
  });

  test("is capped at maxMs", () => {
    expect(backoffDelay(20, { baseMs: 500, factor: 2, maxMs: 30_000 })).toBe(30_000);
  });

  test("attempt 0 or negative yields no delay", () => {
    expect(backoffDelay(0)).toBe(0);
    expect(backoffDelay(-3)).toBe(0);
  });
});

describe("shouldRetry", () => {
  test("retries below the cap, stops at it", () => {
    expect(shouldRetry(0, 5)).toBe(true);
    expect(shouldRetry(4, 5)).toBe(true);
    expect(shouldRetry(5, 5)).toBe(false);
    expect(shouldRetry(6, 5)).toBe(false);
  });
});
