import { describe, expect, test } from "bun:test";
import { buildQuery, dockerignoreExcludes, dockerignoreNegations, mapBuildFrame } from "./build.ts";

describe("buildQuery", () => {
  test("defaults dockerfile and sets rm", () => {
    const q = buildQuery({ contextDir: "/app" });
    expect(q.dockerfile).toBe("Dockerfile");
    expect(q.rm).toBe(true);
    expect(q.t).toBeUndefined();
  });

  test("passes tag, target, and boolean flags", () => {
    const q = buildQuery({
      contextDir: "/app",
      dockerfile: "docker/Dockerfile",
      tag: "myapp:1.0",
      target: "runtime",
      noCache: true,
      pull: true,
    });
    expect(q.dockerfile).toBe("docker/Dockerfile");
    expect(q.t).toBe("myapp:1.0");
    expect(q.target).toBe("runtime");
    expect(q.nocache).toBe(true);
    expect(q.pull).toBe(true);
  });

  test("JSON-encodes build args, omits when empty", () => {
    expect(buildQuery({ contextDir: "/app", buildArgs: {} }).buildargs).toBeUndefined();
    const q = buildQuery({ contextDir: "/app", buildArgs: { VERSION: "2", FOO: "bar" } });
    expect(JSON.parse(q.buildargs as string)).toEqual({ VERSION: "2", FOO: "bar" });
  });

  test("trims whitespace-only optional fields away", () => {
    const q = buildQuery({ contextDir: "/app", tag: "   ", target: "" });
    expect(q.t).toBeUndefined();
    expect(q.target).toBeUndefined();
  });
});

describe("dockerignoreExcludes", () => {
  test("keeps real patterns, drops blanks/comments/negations", () => {
    const text = ["node_modules", "", "# a comment", "  *.log  ", "!keep.me", ".git"].join("\n");
    expect(dockerignoreExcludes(text)).toEqual(["node_modules", "*.log", ".git"]);
  });

  test("empty input yields no excludes", () => {
    expect(dockerignoreExcludes("")).toEqual([]);
  });
});

describe("dockerignoreNegations", () => {
  test("collects negation rules (which the classic builder can't apply)", () => {
    const text = ["*.log", "!keep.me", "node_modules", "  !also.keep  ", "!"].join("\n");
    // The bare "!" is not a real pattern and is excluded.
    expect(dockerignoreNegations(text)).toEqual(["!keep.me", "!also.keep"]);
  });

  test("no negations yields an empty list", () => {
    expect(dockerignoreNegations("node_modules\n*.log")).toEqual([]);
  });
});

describe("mapBuildFrame", () => {
  test("carries stream text", () => {
    expect(mapBuildFrame("r1", { stream: "Step 1/3" })).toEqual({
      requestId: "r1",
      stream: "Step 1/3",
      status: undefined,
      imageId: undefined,
      error: undefined,
      done: false,
    });
  });

  test("extracts the image id from an aux frame", () => {
    expect(mapBuildFrame("r1", { aux: { ID: "sha256:abc" } }).imageId).toBe("sha256:abc");
  });

  test("prefers error, falls back to errorDetail.message", () => {
    expect(mapBuildFrame("r1", { error: "boom" }).error).toBe("boom");
    expect(mapBuildFrame("r1", { errorDetail: { message: "deep" } }).error).toBe("deep");
  });
});
