import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { column, connect, type DB, defineTable, migrate } from "@basket/db";
import { diff, run, status, sync } from "../index.ts";

const v1 = defineTable("notes", {
  id: column.serial().primaryKey(),
  title: column.text(),
});

const v2 = defineTable("notes", {
  id: column.serial().primaryKey(),
  title: column.text(),
  body: column.text().default(""),
  pinned: column.boolean().default(false),
});

let db: DB;
beforeEach(() => {
  db = connect(":memory:");
  migrate(db, [v1]);
});
afterEach(() => db.close());

describe("@basket/migrate", () => {
  test("diff detects new columns", () => {
    const d = diff(db, [v2]);
    expect(d.newTables.length).toBe(0);
    expect(d.newColumns.length).toBe(2);
    expect(d.newColumns.map((c) => c.column).sort()).toEqual(["body", "pinned"]);
  });

  test("sync applies additive changes", () => {
    const before = db.query<{ name: string }>("PRAGMA table_info(notes)").map((r) => r.name);
    expect(before).toEqual(["id", "title"]);

    const report = sync(db, [v2]);
    expect(report.applied.length).toBe(2);

    const after = db.query<{ name: string }>("PRAGMA table_info(notes)").map((r) => r.name);
    expect(after.sort()).toEqual(["body", "id", "pinned", "title"]);
  });

  test("run applies versioned migrations idempotently", () => {
    const migrations = [
      { id: "001", up: "CREATE TABLE tags (id INTEGER PRIMARY KEY, name TEXT)" },
      { id: "002", up: "ALTER TABLE tags ADD COLUMN color TEXT" },
    ];
    expect(run(db, migrations)).toEqual(["001", "002"]);
    expect(run(db, migrations)).toEqual([]);
    expect(status(db, migrations).every((s) => s.appliedAt)).toBe(true);
  });

  test("diff warns on removed columns", () => {
    sync(db, [v2]);
    const d = diff(db, [v1]);
    expect(d.removedColumns.map((c) => c.column).sort()).toEqual(["body", "pinned"]);
  });
});
