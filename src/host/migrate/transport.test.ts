import { describe, expect, test } from "bun:test";
import { frameError } from "./transport.ts";

// Docker's streaming endpoints (/images/load, /images/create) commit HTTP 200
// before the work runs and report failures only as in-band ndjson frames. If we
// miss one of these, a failed migration looks like success — silent data loss.
describe("frameError", () => {
  test("returns undefined for a normal progress frame", () => {
    expect(frameError({ status: "Loading layer", progress: "[==>]" })).toBeUndefined();
    expect(frameError({ stream: "Loaded image: nginx:latest\n" })).toBeUndefined();
  });

  test("detects a top-level error frame", () => {
    expect(frameError({ error: "no space left on device" })).toBe("no space left on device");
  });

  test("detects a nested errorDetail.message frame", () => {
    expect(frameError({ errorDetail: { message: "invalid tar header" }, error: undefined })).toBe(
      "invalid tar header",
    );
  });

  test("ignores a non-string error and a malformed errorDetail", () => {
    expect(frameError({ error: 42 })).toBeUndefined();
    expect(frameError({ errorDetail: {} })).toBeUndefined();
    expect(frameError({ errorDetail: "oops" })).toBeUndefined();
    expect(frameError({})).toBeUndefined();
  });
});
