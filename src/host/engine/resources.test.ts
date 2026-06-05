import { describe, expect, test } from "bun:test";
import { cpus as hostCpus } from "node:os";
import { clampResources, defaultResources } from "./resources.ts";

const cores = hostCpus().length;

describe("clampResources", () => {
  test("fills gaps from defaults", () => {
    expect(clampResources({})).toEqual(defaultResources());
  });

  test("clamps to sane bounds", () => {
    const big = clampResources({ cpus: 9999, memoryGiB: 9999, diskGiB: 99999 });
    expect(big.cpus).toBe(cores); // never more CPUs than the host has
    expect(big.memoryGiB).toBe(64);
    expect(big.diskGiB).toBe(1024);

    const small = clampResources({ cpus: 0, memoryGiB: 0, diskGiB: 1 });
    expect(small.cpus).toBe(1);
    expect(small.memoryGiB).toBe(1);
    expect(small.diskGiB).toBe(8);
  });

  test("floors fractional values", () => {
    const r = clampResources({ cpus: 2.9, memoryGiB: 4.7, diskGiB: 60.5 });
    expect(r.cpus).toBe(Math.min(2, cores));
    expect(r.memoryGiB).toBe(4);
    expect(r.diskGiB).toBe(60);
  });
});
