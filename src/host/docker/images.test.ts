import { describe, expect, test } from "bun:test";
import { splitRef } from "./images.ts";

describe("splitRef", () => {
  test("splits name and tag", () => {
    expect(splitRef("nginx:1.27")).toEqual({ name: "nginx", tag: "1.27" });
    expect(splitRef("ghcr.io/owner/app:dev")).toEqual({ name: "ghcr.io/owner/app", tag: "dev" });
  });

  test("defaults the tag to latest when absent", () => {
    expect(splitRef("nginx")).toEqual({ name: "nginx", tag: "latest" });
    expect(splitRef("owner/app")).toEqual({ name: "owner/app", tag: "latest" });
  });

  test("a registry host:port is not mistaken for a tag", () => {
    expect(splitRef("localhost:5000/app")).toEqual({ name: "localhost:5000/app", tag: "latest" });
    expect(splitRef("localhost:5000/app:v2")).toEqual({
      name: "localhost:5000/app",
      tag: "v2",
    });
  });
});
