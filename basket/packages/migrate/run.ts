import type { DB } from "@basket/db";

export type Migration = {
  readonly id: string;
  readonly up: string | ((db: DB) => void);
  readonly down?: string | ((db: DB) => void);
};

export type MigrationStatus = {
  readonly id: string;
  readonly appliedAt?: string;
};

const TABLE = "__basket_migrations__";

const ensureTable = (db: DB): void => {
  db.raw.exec(`CREATE TABLE IF NOT EXISTS ${TABLE} (
    id TEXT PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
  )`);
};

const appliedIds = (db: DB): Map<string, string> => {
  ensureTable(db);
  const rows = db.query<{ id: string; applied_at: string }>(`SELECT id, applied_at FROM ${TABLE}`);
  return new Map(rows.map((r) => [r.id, r.applied_at]));
};

const apply = (db: DB, m: Migration): void => {
  if (typeof m.up === "string") db.raw.exec(m.up);
  else m.up(db);
};

const revert = (db: DB, m: Migration): void => {
  if (!m.down) throw new Error(`migration ${m.id} has no down`);
  if (typeof m.down === "string") db.raw.exec(m.down);
  else m.down(db);
};

export const run = (db: DB, migrations: readonly Migration[]): readonly string[] => {
  const seen = appliedIds(db);
  const applied: string[] = [];

  db.raw.transaction(() => {
    for (const m of migrations) {
      if (seen.has(m.id)) continue;
      apply(db, m);
      db.raw.query(`INSERT INTO ${TABLE} (id) VALUES (?)`).run(m.id);
      applied.push(m.id);
    }
  })();

  return applied;
};

export const rollback = (db: DB, migrations: readonly Migration[], to?: string): readonly string[] => {
  const seen = appliedIds(db);
  const reverted: string[] = [];
  const ordered = [...migrations].reverse();

  db.raw.transaction(() => {
    for (const m of ordered) {
      if (!seen.has(m.id)) continue;
      if (to && m.id === to) break;
      revert(db, m);
      db.raw.query(`DELETE FROM ${TABLE} WHERE id = ?`).run(m.id);
      reverted.push(m.id);
    }
  })();

  return reverted;
};

export const status = (db: DB, migrations: readonly Migration[]): readonly MigrationStatus[] => {
  const seen = appliedIds(db);
  return migrations.map((m) => ({ id: m.id, appliedAt: seen.get(m.id) }));
};
