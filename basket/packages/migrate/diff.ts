import type { Col, ColumnType, DB, Table } from "@basket/db";
import { tableSql } from "@basket/db";

export type SchemaDiff = {
  readonly newTables: readonly { table: string; sql: string }[];
  readonly newColumns: readonly { table: string; column: string; sql: string }[];
  readonly removedColumns: readonly { table: string; column: string; warn: string }[];
  readonly removedTables: readonly { table: string; warn: string }[];
  readonly typeChanges: readonly { table: string; column: string; warn: string }[];
};

type ExistingCol = {
  readonly name: string;
  readonly type: string;
  readonly notnull: 0 | 1;
  readonly dflt_value: string | null;
  readonly pk: 0 | 1;
};

const existingTables = (db: DB): string[] =>
  db
    .query<{ name: string }>("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
    .map((r) => r.name);

const existingColumns = (db: DB, table: string): ExistingCol[] => db.query<ExistingCol>(`PRAGMA table_info(${table})`);

const sqlType = (t: ColumnType): string => {
  switch (t) {
    case "text":
    case "timestamp":
      return "TEXT";
    case "integer":
    case "boolean":
    case "serial":
      return "INTEGER";
    case "real":
      return "REAL";
    case "blob":
      return "BLOB";
  }
};

const sqlDefault = (v: string | number | boolean | null): string => {
  if (v === null) return "NULL";
  if (typeof v === "string") {
    if (v.toLowerCase() === "now()") return "CURRENT_TIMESTAMP";
    return `'${v.replace(/'/g, "''")}'`;
  }
  if (typeof v === "boolean") return v ? "1" : "0";
  return String(v);
};

const colAlterSql = (name: string, col: Col<unknown>): string => {
  const parts = [name, sqlType(col.type)];
  if (col.defaultValue !== undefined) parts.push(`DEFAULT ${sqlDefault(col.defaultValue)}`);
  if (!col.isNullable && col.defaultValue === undefined) {
    // SQLite cannot ALTER add NOT NULL without default; emit nullable.
  }
  return parts.join(" ");
};

const sameType = (declared: ColumnType, existing: string): boolean => {
  const e = existing.toUpperCase();
  if (declared === "text" || declared === "timestamp") return e === "TEXT";
  if (declared === "integer" || declared === "boolean" || declared === "serial") return e === "INTEGER";
  if (declared === "real") return e === "REAL";
  if (declared === "blob") return e === "BLOB";
  return false;
};

export const diff = (db: DB, tables: readonly Table[]): SchemaDiff => {
  const have = new Set(existingTables(db));
  const declared = new Set(tables.map((t) => t.name));

  const newTables: SchemaDiff["newTables"] = tables
    .filter((t) => !have.has(t.name))
    .map((t) => ({ table: t.name, sql: tableSql(t) }));

  const newColumns: SchemaDiff["newColumns"][number][] = [];
  const removedColumns: SchemaDiff["removedColumns"][number][] = [];
  const typeChanges: SchemaDiff["typeChanges"][number][] = [];

  for (const t of tables) {
    if (!have.has(t.name)) continue;
    const cols = existingColumns(db, t.name);
    const declaredNames = new Set(Object.keys(t.columns));

    for (const [name, col] of Object.entries(t.columns)) {
      const existing = cols.find((c) => c.name === name);
      if (!existing) {
        newColumns.push({
          table: t.name,
          column: name,
          sql: `ALTER TABLE ${t.name} ADD COLUMN ${colAlterSql(name, col as Col<unknown>)}`,
        });
      } else if (!sameType((col as Col<unknown>).type, existing.type)) {
        typeChanges.push({
          table: t.name,
          column: name,
          warn: `type changed (${existing.type} → ${(col as Col<unknown>).type}); SQLite cannot ALTER TYPE — recreate the table`,
        });
      }
    }

    for (const c of cols) {
      if (!declaredNames.has(c.name)) {
        removedColumns.push({
          table: t.name,
          column: c.name,
          warn: `column ${c.name} no longer declared; SQLite ALTER DROP requires recreating the table`,
        });
      }
    }
  }

  const removedTables: SchemaDiff["removedTables"][number][] = [];
  for (const name of have) {
    if (name === "__basket_migrations__") continue;
    if (!declared.has(name)) {
      removedTables.push({ table: name, warn: `table ${name} no longer declared` });
    }
  }

  return { newTables, newColumns, removedColumns, removedTables, typeChanges };
};
