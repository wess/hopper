import { describe, expect, test } from "bun:test";
import { parseDeepLink } from "../index.ts";

describe("parseDeepLink", () => {
  test("extracts scheme, host, path, params", () => {
    const link = parseDeepLink("notes://open/123?focus=true&tab=editor");
    expect(link.scheme).toBe("notes");
    expect(link.host).toBe("open");
    expect(link.path).toBe("/123");
    expect(link.params).toEqual({ focus: "true", tab: "editor" });
  });

  test("preserves raw url", () => {
    expect(parseDeepLink("myapp://foo").url).toBe("myapp://foo");
  });
});
