import { describe, expect, test } from "bun:test";
import {
  baseUrl,
  connectOptions,
  hostHeader,
  normalizePipe,
  parseDockerHost,
  resolveEndpoint,
  unixTarget,
} from "./endpoint.ts";

describe("parseDockerHost", () => {
  test("unix scheme", () => {
    expect(parseDockerHost("unix:///var/run/docker.sock")).toEqual({
      transport: "unix",
      path: "/var/run/docker.sock",
    });
  });

  test("unix scheme with custom path", () => {
    expect(parseDockerHost("unix:///Users/me/.colima/docker.sock")).toEqual({
      transport: "unix",
      path: "/Users/me/.colima/docker.sock",
    });
  });

  test("npipe scheme normalizes to backslashes", () => {
    expect(parseDockerHost("npipe:////./pipe/docker_engine")).toEqual({
      transport: "npipe",
      path: "\\\\.\\pipe\\docker_engine",
    });
  });

  test("tcp scheme with explicit port", () => {
    expect(parseDockerHost("tcp://localhost:2375")).toEqual({
      transport: "tcp",
      host: "localhost",
      port: 2375,
      tls: false,
    });
  });

  test("tcp defaults to 2375, or 2376 when TLS", () => {
    expect(parseDockerHost("tcp://192.168.0.5")).toMatchObject({ port: 2375, tls: false });
    expect(parseDockerHost("tcp://192.168.0.5", true)).toMatchObject({ port: 2376, tls: true });
  });

  test("https scheme implies TLS", () => {
    expect(parseDockerHost("https://docker.example.com:2376")).toEqual({
      transport: "tcp",
      host: "docker.example.com",
      port: 2376,
      tls: true,
    });
  });

  test("IPv6 literal with port", () => {
    expect(parseDockerHost("tcp://[::1]:2375")).toEqual({
      transport: "tcp",
      host: "::1",
      port: 2375,
      tls: false,
    });
  });

  test("bare unix path and bare windows pipe", () => {
    expect(parseDockerHost("/var/run/docker.sock")).toEqual({
      transport: "unix",
      path: "/var/run/docker.sock",
    });
    expect(parseDockerHost("//./pipe/docker_engine")).toEqual({
      transport: "npipe",
      path: "\\\\.\\pipe\\docker_engine",
    });
  });

  test("unrecognized / empty returns null", () => {
    expect(parseDockerHost("")).toBeNull();
    expect(parseDockerHost("ftp://nope")).toBeNull();
    expect(parseDockerHost("garbage")).toBeNull();
  });
});

describe("resolveEndpoint", () => {
  test("DOCKER_HOST wins over everything", () => {
    expect(
      resolveEndpoint({ DOCKER_HOST: "tcp://10.0.0.2:2376", DOCKER_SOCKET: "/x.sock" }, "linux"),
    ).toMatchObject({ transport: "tcp", host: "10.0.0.2", port: 2376 });
  });

  test("DOCKER_TLS_VERIFY upgrades a tcp DOCKER_HOST", () => {
    expect(
      resolveEndpoint({ DOCKER_HOST: "tcp://h", DOCKER_TLS_VERIFY: "1" }, "linux"),
    ).toMatchObject({ transport: "tcp", tls: true, port: 2376 });
  });

  test("falls back to DOCKER_SOCKET (unix or pipe)", () => {
    expect(resolveEndpoint({ DOCKER_SOCKET: "/run/user/1000/docker.sock" }, "linux")).toEqual({
      transport: "unix",
      path: "/run/user/1000/docker.sock",
    });
    expect(resolveEndpoint({ DOCKER_SOCKET: "//./pipe/custom" }, "win32")).toEqual({
      transport: "npipe",
      path: "\\\\.\\pipe\\custom",
    });
  });

  test("platform default: unix on linux/mac, named pipe on windows", () => {
    expect(resolveEndpoint({}, "linux")).toEqual({
      transport: "unix",
      path: "/var/run/docker.sock",
    });
    expect(resolveEndpoint({}, "darwin")).toEqual({
      transport: "unix",
      path: "/var/run/docker.sock",
    });
    expect(resolveEndpoint({}, "win32")).toEqual({
      transport: "npipe",
      path: "\\\\.\\pipe\\docker_engine",
    });
  });

  test("a bad DOCKER_HOST falls through instead of connecting wrong", () => {
    expect(resolveEndpoint({ DOCKER_HOST: "garbage" }, "linux")).toEqual({
      transport: "unix",
      path: "/var/run/docker.sock",
    });
  });
});

describe("adapters", () => {
  const unix = { transport: "unix", path: "/var/run/docker.sock" } as const;
  const tcp = { transport: "tcp", host: "h", port: 2376, tls: true } as const;
  const pipe = { transport: "npipe", path: "\\\\.\\pipe\\docker_engine" } as const;

  test("baseUrl", () => {
    expect(baseUrl(unix, "v1.43")).toBe("http://localhost/v1.43");
    expect(baseUrl(tcp, "v1.43")).toBe("https://h:2376/v1.43");
    expect(baseUrl({ ...tcp, tls: false }, "v1.43")).toBe("http://h:2376/v1.43");
  });

  test("unixTarget is the path for socket/pipe, undefined for tcp", () => {
    expect(unixTarget(unix)).toBe("/var/run/docker.sock");
    expect(unixTarget(pipe)).toBe("\\\\.\\pipe\\docker_engine");
    expect(unixTarget(tcp)).toBeUndefined();
  });

  test("connectOptions shapes for Bun.connect", () => {
    expect(connectOptions(unix)).toEqual({ unix: "/var/run/docker.sock" });
    expect(connectOptions(tcp)).toEqual({ hostname: "h", port: 2376, tls: true });
  });

  test("hostHeader", () => {
    expect(hostHeader(unix)).toBe("localhost");
    expect(hostHeader(tcp)).toBe("h:2376");
  });
});

describe("normalizePipe", () => {
  test("forward slashes become backslashes; idempotent on backslashes", () => {
    expect(normalizePipe("//./pipe/docker_engine")).toBe("\\\\.\\pipe\\docker_engine");
    expect(normalizePipe("\\\\.\\pipe\\docker_engine")).toBe("\\\\.\\pipe\\docker_engine");
  });
});
