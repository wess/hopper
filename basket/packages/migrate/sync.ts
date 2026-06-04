import type { DB, Table } from "@basket/db";
import { diff } from "./diff.ts";

export type SyncReport = {
  readonly applied: readonly string[];
  readonly warnings: readonly string[];
};

export const sync = (db: DB, tables: readonly Table[]): SyncReport => {
  const d = diff(db, tables);
  const applied: string[] = [];
  const warnings: string[] = [];

  db.raw.transaction(() => {
    for (const t of d.newTables) {
      db.raw.exec(t.sql);
      applied.push(`+ table ${t.table}`);
    }
    for (const c of d.newColumns) {
      db.raw.exec(c.sql);
      applied.push(`+ ${c.table}.${c.column}`);
    }
  })();

  for (const w of d.removedColumns) warnings.push(w.warn);
  for (const w of d.removedTables) warnings.push(w.warn);
  for (const w of d.typeChanges) warnings.push(w.warn);

  return { applied, warnings };
};
