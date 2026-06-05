import { describe, expect, test } from "bun:test";
import { classify, type ProbeResult, toState } from "./diagnose.ts";

describe("classify", () => {
  test("ENOENT → absent", () => {
    expect(classify({ code: "ENOENT", message: "no such file" }).reason).toBe("absent");
  });
  test("ECONNREFUSED → refused", () => {
    expect(classify({ code: "ECONNREFUSED", message: "x" }).reason).toBe("refused");
  });
  test("EACCES → denied", () => {
    expect(classify({ code: "EACCES", message: "x" }).reason).toBe("denied");
  });
  test("abort → timeout", () => {
    expect(classify({ name: "AbortError", message: "aborted" }).reason).toBe("timeout");
  });
  test("unknown → error", () => {
    expect(classify(new Error("something weird")).reason).toBe("error");
  });
});

describe("toState", () => {
  const fail = (reason: "absent" | "refused" | "denied" | "timeout" | "error"): ProbeResult => ({
    ok: false,
    reason,
    detail: "",
  });
  test("ok → connected", () => {
    expect(toState({ ok: true }, { managed: false })).toBe("connected");
  });
  test("absent depends on managed", () => {
    expect(toState(fail("absent"), { managed: true })).toBe("stopped");
    expect(toState(fail("absent"), { managed: false })).toBe("notInstalled");
  });
  test("refused → stopped", () => {
    expect(toState(fail("refused"), { managed: false })).toBe("stopped");
  });
  test("denied → needsPermission", () => {
    expect(toState(fail("denied"), { managed: true })).toBe("needsPermission");
  });
  test("timeout/error → unreachable", () => {
    expect(toState(fail("timeout"), { managed: true })).toBe("unreachable");
    expect(toState(fail("error"), { managed: false })).toBe("unreachable");
  });
});
