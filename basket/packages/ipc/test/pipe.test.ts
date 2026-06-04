import { describe, expect, test } from "bun:test";
import { assign, pipeline, validate } from "../index.ts";

describe("pipeline", () => {
  test("threads input through pipes", async () => {
    const addOne = (ctx: { input: number; assigns: Record<string, unknown> }) => ({
      ...ctx,
      input: ctx.input + 1,
    });
    const handler = pipeline<number>(addOne, addOne)(async ({ input }) => input * 10);
    expect(await handler(1)).toBe(30);
  });

  test("validate runs schema.parse", async () => {
    const schema = {
      parse: (v: unknown) => {
        if (typeof v !== "string") throw new Error("expected string");
        return v.trim();
      },
    };
    const handler = pipeline<string>(validate(schema))(async ({ input }) => input.length);
    expect(await handler("  hi  ")).toBe(2);
  });

  test("assign adds typed assigns", async () => {
    const setUser = async (ctx: { input: number; assigns: Record<string, unknown> }) => ({
      ...ctx,
      assigns: { ...ctx.assigns, user: { id: 42 } },
    });

    const handler = pipeline<number>(setUser)(({ assigns }) => (assigns.user as { id: number }).id);
    expect(await handler(0)).toBe(42);
  });

  test("assign helper composes purely", () => {
    const next = assign({ input: 1, assigns: {} as Record<string, unknown> }, "k", "v");
    expect(next.assigns.k).toBe("v");
    expect(next.input).toBe(1);
  });
});
