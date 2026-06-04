import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { column, connect, type DB, defineTable, from, migrate, type RowOf } from "../index.ts";

const notes = defineTable("notes", {
  id: column.serial().primaryKey(),
  title: column.text(),
  body: column.text().default(""),
  pinned: column.boolean().default(false),
});
type Note = RowOf<typeof notes>;

let db: DB;

beforeEach(() => {
  db = connect(":memory:");
  migrate(db, [notes]);
});

afterEach(() => {
  db.close();
});

describe("@basket/db", () => {
  test("insert + all round-trip", () => {
    const created = db.insert(notes, { title: "hi", body: "yo" });
    expect(created.id).toBe(1);
    expect(created.title).toBe("hi");
    const all = db.all(from(notes));
    expect(all.length).toBe(1);
    expect(all[0]?.title).toBe("hi");
  });

  test("where filters", () => {
    db.insert(notes, { title: "a" });
    db.insert(notes, { title: "b", pinned: true });
    const pinned = db.all(from(notes).where((q) => q("pinned").equals(true)));
    expect(pinned.length).toBe(1);
    expect(pinned[0]?.title).toBe("b");
  });

  test("select projection", () => {
    db.insert(notes, { title: "x" });
    const titles = db.all<Pick<Note, "id" | "title">>(from(notes).select("id", "title"));
    expect(Object.keys(titles[0] ?? {}).sort()).toEqual(["id", "title"]);
  });

  test("update + remove", () => {
    const n = db.insert(notes, { title: "x" });
    const updated = db.update(notes, { id: n.id }, { title: "y" });
    expect(updated).toBe(1);
    const after = db.one(from(notes).where((q) => q("id").equals(n.id)));
    expect(after?.title).toBe("y");
    const removed = db.remove(notes, { id: n.id });
    expect(removed).toBe(1);
  });

  test("order + limit", () => {
    db.insert(notes, { title: "a" });
    db.insert(notes, { title: "b" });
    db.insert(notes, { title: "c" });
    const r = db.all(from(notes).order("id", "desc").limit(2));
    expect(r.map((n) => n.title)).toEqual(["c", "b"]);
  });
});
