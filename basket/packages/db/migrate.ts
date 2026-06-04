import type { Col, ColumnType } from "./column.ts";
import type { DB } from "./connect.ts";
import type { Table } from "./table.ts";

const sqlType = (t: ColumnType): string => {
  switch (t) {
    case "text":
      return "TEXT";
    case "integer":
      return "INTEGER";
    case "real":
      return "REAL";
    case "blob":
      return "BLOB";
    case "boolean":
      return "INTEGER";
    case "timestamp":
      return "TEXT";
    case "serial":
      return "INTEGER";
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

const colSql = (name: string, col: Col<unknown>): string => {
  const parts = [name, sqlType(col.type)];
  if (col.type === "serial" || col.isPrimaryKey) {
    parts.push("PRIMARY KEY");
    if (col.type === "serial") parts.push("AUTOINCREMENT");
  }
  if (!col.isNullable && !col.isPrimaryKey && col.type !== "serial") parts.push("NOT NULL");
  if (col.isUnique && !col.isPrimaryKey) parts.push("UNIQUE");
  if (col.defaultValue !== undefined) parts.push(`DEFAULT ${sqlDefault(col.defaultValue)}`);
  return parts.join(" ");
};

export const tableSql = (table: Table): string => {
  const cols = Object.entries(table.columns).map(([name, c]) => colSql(name, c as Col<unknown>));
  return `CREATE TABLE IF NOT EXISTS ${table.name} (\n  ${cols.join(",\n  ")}\n)`;
};

export const migrate = (db: DB, tables: readonly Table[]): void => {
  db.raw.transaction(() => {
    for (const t of tables) {
      db.raw.exec(tableSql(t));
    }
  })();
};
