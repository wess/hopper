import { describe, expect, test } from "bun:test";
import { hardenedHostConfig, sandboxConfig } from "./sandbox.ts";

describe("sandboxConfig", () => {
  test("safe defaults with an empty env", () => {
    const cfg = sandboxConfig({});
    expect(cfg.dockerHost).toBeUndefined();
    expect(cfg.network).toBe("none");
    expect(cfg.pidsLimit).toBe(256);
    expect(cfg.memoryBytes).toBe(512 * 1024 * 1024);
    expect(cfg.nanoCpus).toBe(1_000_000_000);
    expect(cfg.autoRemove).toBe(true);
  });

  test("'host' network is never honored (re-exposes the host stack)", () => {
    expect(sandboxConfig({ HOPPER_MCP_NETWORK: "host" }).network).toBe("none");
    expect(sandboxConfig({ HOPPER_MCP_NETWORK: "HOST" }).network).toBe("none");
    expect(sandboxConfig({ HOPPER_MCP_NETWORK: "   " }).network).toBe("none");
    expect(sandboxConfig({ HOPPER_MCP_NETWORK: "internal" }).network).toBe("internal");
  });

  test("numeric env overrides, ignoring non-positive / non-finite junk", () => {
    const cfg = sandboxConfig({
      HOPPER_MCP_PIDS_LIMIT: "64",
      HOPPER_MCP_MEMORY_MB: "256",
      HOPPER_MCP_CPUS: "2",
    });
    expect(cfg.pidsLimit).toBe(64);
    expect(cfg.memoryBytes).toBe(256 * 1024 * 1024);
    expect(cfg.nanoCpus).toBe(2_000_000_000);

    const bad = sandboxConfig({ HOPPER_MCP_PIDS_LIMIT: "-5", HOPPER_MCP_MEMORY_MB: "nope" });
    expect(bad.pidsLimit).toBe(256);
    expect(bad.memoryBytes).toBe(512 * 1024 * 1024);
  });

  test("autoRemove can be disabled only with explicit false-y values", () => {
    expect(sandboxConfig({ HOPPER_MCP_AUTOREMOVE: "0" }).autoRemove).toBe(false);
    expect(sandboxConfig({ HOPPER_MCP_AUTOREMOVE: "false" }).autoRemove).toBe(false);
    expect(sandboxConfig({ HOPPER_MCP_AUTOREMOVE: "no" }).autoRemove).toBe(false);
    expect(sandboxConfig({ HOPPER_MCP_AUTOREMOVE: "1" }).autoRemove).toBe(true);
  });
});

describe("hardenedHostConfig", () => {
  test("non-negotiable isolation flags are always present", () => {
    const hc = hardenedHostConfig(sandboxConfig({}));
    expect(hc.Binds).toEqual([]);
    expect(hc.Mounts).toEqual([]);
    expect(hc.CapDrop).toEqual(["ALL"]);
    expect(hc.CapAdd).toEqual([]);
    expect(hc.SecurityOpt).toEqual(["no-new-privileges"]);
    expect(hc.Privileged).toBe(false);
    expect(hc.NetworkMode).toBe("none");
  });

  test("resource ceilings flow from the config", () => {
    const hc = hardenedHostConfig(
      sandboxConfig({ HOPPER_MCP_PIDS_LIMIT: "100", HOPPER_MCP_MEMORY_MB: "128" }),
    );
    expect(hc.PidsLimit).toBe(100);
    expect(hc.Memory).toBe(128 * 1024 * 1024);
  });
});
