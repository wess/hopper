import { describe, expect, test } from "bun:test";
import { composeArgs } from "./compose.ts";

describe("composeArgs", () => {
  test("up runs detached and removes orphans", () => {
    expect(composeArgs("up", "/p/compose.yml")).toEqual([
      "compose",
      "-f",
      "/p/compose.yml",
      "up",
      "-d",
      "--remove-orphans",
    ]);
  });

  test("down tears the stack down", () => {
    expect(composeArgs("down", "/p/compose.yml")).toEqual([
      "compose",
      "-f",
      "/p/compose.yml",
      "down",
    ]);
  });

  test("passes a project name when supplied", () => {
    expect(composeArgs("up", "c.yml", "myproj")).toEqual([
      "compose",
      "-f",
      "c.yml",
      "-p",
      "myproj",
      "up",
      "-d",
      "--remove-orphans",
    ]);
  });

  test("ignores a blank project name", () => {
    expect(composeArgs("down", "c.yml", "   ")).toEqual(["compose", "-f", "c.yml", "down"]);
  });
});
