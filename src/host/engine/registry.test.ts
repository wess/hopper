import { describe, expect, test } from "bun:test";
import type { Provider } from "./provider.ts";
import { candidatesFor, messageFor, selectProvider } from "./registry.ts";

const fake = (id: string, available: boolean, managed = false): Provider => ({
  id,
  label: id.toUpperCase(),
  managed,
  available: async () => available,
  status: async () => ({ state: "stopped", detail: "", endpoint: null }),
});

describe("candidatesFor", () => {
  test("macOS prefers the VM engine, then external", () => {
    expect(candidatesFor("darwin").map((p) => p.id)).toEqual(["vz", "existing"]);
  });
  test("linux prefers native dockerd, then external", () => {
    expect(candidatesFor("linux").map((p) => p.id)).toEqual(["linux", "existing"]);
  });
  test("other platforms get the external fallback only", () => {
    expect(candidatesFor("win32").map((p) => p.id)).toEqual(["existing"]);
  });
});

describe("selectProvider", () => {
  test("picks the first available candidate", async () => {
    const got = await selectProvider([fake("vz", false), fake("existing", true)], null);
    expect(got.id).toBe("existing");
  });
  test("an available preference wins over order", async () => {
    const got = await selectProvider([fake("vz", true), fake("existing", true)], "existing");
    expect(got.id).toBe("existing");
  });
  test("an unavailable preference is ignored", async () => {
    const got = await selectProvider([fake("vz", false), fake("existing", true)], "vz");
    expect(got.id).toBe("existing");
  });
  test("falls back to the last candidate when none are available", async () => {
    const got = await selectProvider([fake("vz", false), fake("existing", false)], null);
    expect(got.id).toBe("existing");
  });
});

describe("messageFor", () => {
  const managed = fake("vz", true, true);
  test("connected is generic", () => expect(messageFor("connected", managed)).toMatch(/Connected/));
  test("starting names the provider", () =>
    expect(messageFor("starting", managed)).toContain(managed.label));
  test("a managed stopped engine names the provider", () =>
    expect(messageFor("stopped", managed)).toContain(managed.label));
  test("an unmanaged stopped engine is generic", () =>
    expect(messageFor("stopped", fake("existing", true, false))).toMatch(/not running/));
  test("needsPermission is explained", () =>
    expect(messageFor("needsPermission", managed)).toMatch(/Permission/));
});
