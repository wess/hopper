import { describe, expect, test } from "bun:test";
import { lineSplitter } from "./control.ts";

describe("lineSplitter", () => {
  test("splits complete lines and buffers the remainder", () => {
    const split = lineSplitter();
    expect(split('{"a":1}\n{"b":2}\n')).toEqual(['{"a":1}', '{"b":2}']);
  });

  test("holds a partial line until its newline arrives", () => {
    const split = lineSplitter();
    expect(split('{"a"')).toEqual([]);
    expect(split(":1}\n")).toEqual(['{"a":1}']);
  });

  test("reassembles a line split across many chunks", () => {
    const split = lineSplitter();
    expect(split("hel")).toEqual([]);
    expect(split("lo")).toEqual([]);
    expect(split("\nworld\n")).toEqual(["hello", "world"]);
  });

  test("no newline yields nothing", () => {
    const split = lineSplitter();
    expect(split("partial")).toEqual([]);
  });
});
