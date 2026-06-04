import { describe, expect, test } from "bun:test";
import { parseArgs } from "../command/index.ts";

describe("parseArgs", () => {
  test("parses long flags", () => {
    const r = parseArgs(["--name", "foo"], { name: { type: "string" } });
    expect(r.flags.name).toBe("foo");
  });

  test("parses short flags", () => {
    const r = parseArgs(["-n", "foo"], { name: { short: "n", type: "string" } });
    expect(r.flags.name).toBe("foo");
  });

  test("boolean flags don't consume next arg", () => {
    const r = parseArgs(["--yes", "extra"], { yes: { type: "boolean" } });
    expect(r.flags.yes).toBe(true);
    expect(r.args).toEqual(["extra"]);
  });

  test("number flags coerce", () => {
    const r = parseArgs(["--port", "3000"], { port: { type: "number" } });
    expect(r.flags.port).toBe(3000);
  });

  test("defaults applied when flag missing", () => {
    const r = parseArgs([], { template: { type: "string", default: "minimal" } });
    expect(r.flags.template).toBe("minimal");
  });

  test("positional args accumulate", () => {
    const r = parseArgs(["foo", "bar"], {});
    expect(r.args).toEqual(["foo", "bar"]);
  });
});
