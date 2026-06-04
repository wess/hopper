import { describe, expect, test } from "bun:test";
import { compareVersions, isNewer, parseVersion } from "../index.ts";

describe("version", () => {
  test("parseVersion", () => {
    expect(parseVersion("1.2.3")).toEqual([1, 2, 3]);
    expect(parseVersion("v1.0")).toEqual([1, 0]);
  });

  test("compareVersions", () => {
    expect(compareVersions("1.0.0", "1.0.0")).toBe(0);
    expect(compareVersions("1.0.1", "1.0.0")).toBe(1);
    expect(compareVersions("1.0.0", "1.1.0")).toBe(-1);
    expect(compareVersions("2.0", "1.99.99")).toBe(1);
  });

  test("isNewer", () => {
    expect(isNewer("1.0.1", "1.0.0")).toBe(true);
    expect(isNewer("1.0.0", "1.0.0")).toBe(false);
    expect(isNewer("0.9.0", "1.0.0")).toBe(false);
  });
});
