import { describe, expect, test } from "bun:test";
import {
  decodeAuthEntry,
  encodeRegistryAuth,
  matchKey,
  parseDockerConfig,
  registryHost,
} from "./credentials.ts";

const HUB = "https://index.docker.io/v1/";

describe("registryHost", () => {
  test("bare names map to Docker Hub", () => {
    expect(registryHost("nginx")).toBe(HUB);
    expect(registryHost("library/postgres")).toBe(HUB);
    expect(registryHost("owner/app:tag")).toBe(HUB);
  });

  test("explicit docker.io maps to Hub", () => {
    expect(registryHost("docker.io/owner/app")).toBe(HUB);
  });

  test("a dotted/port/localhost first segment is the registry host", () => {
    expect(registryHost("ghcr.io/owner/app:1.0")).toBe("ghcr.io");
    expect(registryHost("quay.io/org/img")).toBe("quay.io");
    expect(registryHost("localhost:5000/img")).toBe("localhost:5000");
    expect(registryHost("registry.example.com/team/app")).toBe("registry.example.com");
  });
});

describe("parseDockerConfig", () => {
  test("parses valid json", () => {
    expect(parseDockerConfig('{"credsStore":"osxkeychain"}').credsStore).toBe("osxkeychain");
  });
  test("returns an empty config on garbage", () => {
    expect(parseDockerConfig("not json")).toEqual({});
  });
});

describe("encode/decode auth", () => {
  test("round-trips a base64 user:pass", () => {
    const b64 = Buffer.from("alice:s3cret").toString("base64");
    expect(decodeAuthEntry(b64)).toEqual({ username: "alice", password: "s3cret" });
  });

  test("passwords may contain colons", () => {
    const b64 = Buffer.from("bob:a:b:c").toString("base64");
    expect(decodeAuthEntry(b64)).toEqual({ username: "bob", password: "a:b:c" });
  });

  test("encodeRegistryAuth produces decodable base64 json", () => {
    const header = encodeRegistryAuth({ username: "u", password: "p", serveraddress: HUB });
    expect(JSON.parse(Buffer.from(header, "base64").toString("utf8"))).toEqual({
      username: "u",
      password: "p",
      serveraddress: HUB,
    });
  });
});

describe("matchKey", () => {
  test("matches exact and scheme/slash-insensitive keys", () => {
    expect(matchKey([HUB, "ghcr.io"], HUB)).toBe(HUB);
    expect(matchKey(["ghcr.io"], "ghcr.io")).toBe("ghcr.io");
    expect(matchKey(["https://ghcr.io/"], "ghcr.io")).toBe("https://ghcr.io/");
    expect(matchKey(["quay.io"], "ghcr.io")).toBeUndefined();
  });
});
