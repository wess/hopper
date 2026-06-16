import { describe, expect, test } from "bun:test";
import { contextCommands } from "./cli.ts";

describe("docker cli integration", () => {
  test("creates a Hopper context when one does not exist", () => {
    expect(contextCommands("unix:///Users/me/.hopper/run/docker.sock", false)).toEqual([
      [
        "context",
        "create",
        "hopper",
        "--description",
        "Hopper managed Docker engine",
        "--docker",
        "host=unix:///Users/me/.hopper/run/docker.sock",
      ],
      ["context", "use", "hopper"],
    ]);
  });

  test("updates the Hopper context when it already exists", () => {
    expect(contextCommands("unix:///tmp/docker.sock", true)).toEqual([
      ["context", "update", "hopper", "--docker", "host=unix:///tmp/docker.sock"],
      ["context", "use", "hopper"],
    ]);
  });
});
