import { describe, expect, test } from "bun:test";
import type { Container } from "../../shared/types.ts";
import { stackState } from "./compose.ts";

const c = (state: Container["state"]): Container =>
  ({ id: Math.random().toString(), state }) as Container;

describe("stackState", () => {
  test("all running → running", () => {
    expect(stackState([c("running"), c("running")])).toEqual({
      running: 2,
      total: 2,
      state: "running",
    });
  });

  test("none running → stopped", () => {
    expect(stackState([c("exited"), c("created")])).toEqual({
      running: 0,
      total: 2,
      state: "stopped",
    });
  });

  test("some running → partial", () => {
    expect(stackState([c("running"), c("exited"), c("paused")])).toEqual({
      running: 1,
      total: 3,
      state: "partial",
    });
  });
});
