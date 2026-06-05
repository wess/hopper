import { describe, expect, test } from "bun:test";
import type { Endpoint } from "../../docker/endpoint.ts";
import { candidateSockets } from "./existing.ts";

const paths = (eps: Endpoint[]): string[] =>
  eps.map((e) => (e.transport === "tcp" ? `tcp:${e.host}:${e.port}` : e.path));

describe("candidateSockets", () => {
  test("a tcp/npipe base is used verbatim (no sweep)", () => {
    const base: Endpoint = { transport: "tcp", host: "h", port: 2375, tls: false };
    expect(candidateSockets(base)).toEqual([base]);
  });

  test("unix base leads, then the known Desktop-free sockets", () => {
    const got = paths(
      candidateSockets(
        { transport: "unix", path: "/custom.sock" },
        "/home/u",
        1000,
        "/run/user/1000",
      ),
    );
    expect(got[0]).toBe("/custom.sock");
    expect(got).toContain("/home/u/.colima/default/docker.sock");
    expect(got).toContain("/home/u/.orbstack/run/docker.sock");
    expect(got).toContain("/run/user/1000/docker.sock");
    expect(got).toContain("/run/user/1000/podman/podman.sock");
  });

  test("de-dupes when the base equals a known path", () => {
    const got = paths(
      candidateSockets(
        { transport: "unix", path: "/home/u/.colima/default/docker.sock" },
        "/home/u",
        null,
      ),
    );
    expect(got.filter((p) => p === "/home/u/.colima/default/docker.sock")).toHaveLength(1);
  });

  test("omits uid-based sockets when uid is unknown", () => {
    const got = paths(
      candidateSockets({ transport: "unix", path: "/x.sock" }, "/home/u", null, undefined),
    );
    expect(got.some((p) => p.startsWith("/run/user/"))).toBe(false);
  });
});
