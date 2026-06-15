import { describe, expect, test } from "bun:test";
import { composeArgs } from "./args.ts";

describe("composeArgs", () => {
  test("up runs detached and removes orphans by default", () => {
    expect(composeArgs("up", { files: ["/p/compose.yml"] })).toEqual([
      "compose",
      "-f",
      "/p/compose.yml",
      "up",
      "-d",
      "--remove-orphans",
    ]);
  });

  test("down tears the stack down", () => {
    expect(composeArgs("down", { files: ["/p/compose.yml"] })).toEqual([
      "compose",
      "-f",
      "/p/compose.yml",
      "down",
    ]);
  });

  test("passes a project name and supports label-driven ops with no file", () => {
    expect(composeArgs("stop", { project: "myproj" })).toEqual(["compose", "-p", "myproj", "stop"]);
  });

  test("ignores blank project / file entries", () => {
    expect(composeArgs("restart", { files: ["   "], project: "  " })).toEqual([
      "compose",
      "restart",
    ]);
  });

  test("up with multiple files, env-file, profiles, build and force-recreate", () => {
    expect(
      composeArgs(
        "up",
        { files: ["a.yml", "b.yml"], project: "app", envFile: ".env" },
        { profiles: ["dev", "debug"], build: true, forceRecreate: true },
      ),
    ).toEqual([
      "compose",
      "-f",
      "a.yml",
      "-f",
      "b.yml",
      "-p",
      "app",
      "--env-file",
      ".env",
      "--profile",
      "dev",
      "--profile",
      "debug",
      "up",
      "-d",
      "--remove-orphans",
      "--build",
      "--force-recreate",
    ]);
  });

  test("up can opt out of --remove-orphans", () => {
    expect(composeArgs("up", { files: ["c.yml"] }, { removeOrphans: false })).toEqual([
      "compose",
      "-f",
      "c.yml",
      "up",
      "-d",
    ]);
  });

  test("down with volumes, orphans and rmi", () => {
    expect(
      composeArgs("down", { project: "app" }, { volumes: true, removeOrphans: true, rmi: "all" }),
    ).toEqual(["compose", "-p", "app", "down", "--volumes", "--remove-orphans", "--rmi", "all"]);
  });

  test("remove is a full teardown", () => {
    expect(composeArgs("remove", { project: "app" })).toEqual([
      "compose",
      "-p",
      "app",
      "down",
      "--volumes",
      "--remove-orphans",
    ]);
  });
});
