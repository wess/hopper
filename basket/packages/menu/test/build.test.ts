import { describe, expect, test } from "bun:test";
import { item, section, separator } from "../build.ts";

describe("menu builders", () => {
  test("item with shortcut", () => {
    expect(item("Save", "save", { shortcut: "CmdOrCtrl+S" })).toEqual({
      label: "Save",
      action: "save",
      shortcut: "CmdOrCtrl+S",
    });
  });

  test("item without shortcut omits it", () => {
    const it = item("X", "x");
    expect("shortcut" in it).toBe(false);
  });

  test("separator", () => {
    expect(separator()).toEqual({ separator: true });
  });

  test("section composes", () => {
    const s = section("File", [item("Quit", "quit"), separator()]);
    expect(s.label).toBe("File");
    expect(s.items.length).toBe(2);
  });
});
