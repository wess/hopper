import { describe, expect, test } from "bun:test";
import { isRawBody } from "./client.ts";

describe("isRawBody", () => {
  test("true for binary/streaming payloads", () => {
    expect(isRawBody(new Uint8Array([1, 2]))).toBe(true);
    expect(isRawBody(new ArrayBuffer(4))).toBe(true);
    expect(isRawBody(new Blob(["x"]))).toBe(true);
    expect(isRawBody(new ReadableStream())).toBe(true);
  });

  test("false for plain objects (JSON bodies)", () => {
    expect(isRawBody({ a: 1 })).toBe(false);
    expect(isRawBody([1, 2, 3])).toBe(false);
    expect(isRawBody("string")).toBe(false);
    expect(isRawBody(undefined)).toBe(false);
  });
});
