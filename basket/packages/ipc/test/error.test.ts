import { describe, expect, test } from "bun:test";
import { decodeError, encodeError, notFound, unauthorized } from "../index.ts";

describe("ipc errors", () => {
  test("notFound encodes and decodes", () => {
    const encoded = encodeError(notFound("user"));
    const shape = decodeError(encoded);
    expect(shape).toEqual({ code: "not_found", message: "user not found" });
  });

  test("unauthorized custom message", () => {
    const encoded = encodeError(unauthorized("bad token"));
    const shape = decodeError(encoded);
    expect(shape?.code).toBe("unauthorized");
    expect(shape?.message).toBe("bad token");
  });

  test("decode returns undefined for non-ipc errors", () => {
    expect(decodeError(new Error("plain"))).toBeUndefined();
  });

  test("encode falls back to internal for unknown throwables", () => {
    const shape = decodeError(encodeError("string error"));
    expect(shape?.code).toBe("internal");
  });
});
