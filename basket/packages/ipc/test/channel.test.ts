import { describe, expect, test } from "bun:test";
import { defineChannel, defineEvent } from "../channel.ts";

describe("channels", () => {
  test("defineChannel returns named object", () => {
    const c = defineChannel<{ a: number }, string>("foo");
    expect(c.name).toBe("foo");
  });

  test("defineEvent returns named object", () => {
    const e = defineEvent<{ x: 1 }>("bar");
    expect(e.name).toBe("bar");
  });
});
