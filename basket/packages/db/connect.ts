import { Database } from "bun:sqlite";
import { type Query, renderQuery } from "./query.ts";
import type { RowOf, Table } from "./table.ts";

export type DB = {
  readonly raw: Database;
  readonly all: <R>(q: Query<R>) => R[];
  readonly one: <R>(q: Query<R>) => R | undefined;
  readonly exec: (sql: string, ...params: unknown[]) => void;
  readonly query: <R>(sql: string, ...params: unknown[]) => R[];
  readonly insert: <T extends Table>(table: T, data: Partial<RowOf<T>>) => RowOf<T>;
  readonly update: <T extends Table>(table: T, where: Partial<RowOf<T>>, data: Partial<RowOf<T>>) => number;
  readonly remove: <T extends Table>(table: T, where: Partial<RowOf<T>>) => number;
  readonly close: () => void;
};

const eqWhere = (where: Record<string, unknown>): { sql: string; params: unknown[] } => {
  const keys = Object.keys(where);
  if (keys.length === 0) return { sql: "", params: [] };
  const params = keys.map((k) => where[k]);
  const sql = ` WHERE ${keys.map((k) => `${k} = ?`).join(" AND ")}`;
  return { sql, params };
};

export const connect = (path: string): DB => {
  const db = new Database(path);
  db.exec("PRAGMA journal_mode = WAL");
  db.exec("PRAGMA foreign_keys = ON");

  return {
    raw: db,

    all: <R>(q: Query<R>) => {
      const { sql, params } = renderQuery(q);
      return db.query(sql).all(...(params as never[])) as R[];
    },

    one: <R>(q: Query<R>) => {
      const { sql, params } = renderQuery(q);
      const row = db.query(sql).get(...(params as never[]));
      return (row ?? undefined) as R | undefined;
    },

    exec: (sql, ...params) => {
      db.query(sql).run(...(params as never[]));
    },

    query: <R>(sql: string, ...params: unknown[]) => {
      return db.query(sql).all(...(params as never[])) as R[];
    },

    insert: <T extends Table>(table: T, data: Partial<RowOf<T>>) => {
      const keys = Object.keys(data);
      const placeholders = keys.map(() => "?").join(", ");
      const values = keys.map((k) => (data as Record<string, unknown>)[k]);
      const sql = `INSERT INTO ${table.name} (${keys.join(", ")}) VALUES (${placeholders}) RETURNING *`;
      const row = db.query(sql).get(...(values as never[]));
      return row as RowOf<T>;
    },

    update: <T extends Table>(table: T, where: Partial<RowOf<T>>, data: Partial<RowOf<T>>) => {
      const setKeys = Object.keys(data);
      const setSql = setKeys.map((k) => `${k} = ?`).join(", ");
      const setVals = setKeys.map((k) => (data as Record<string, unknown>)[k]);
      const w = eqWhere(where as Record<string, unknown>);
      const result = db
        .query(`UPDATE ${table.name} SET ${setSql}${w.sql}`)
        .run(...([...setVals, ...w.params] as never[]));
      return Number(result.changes ?? 0);
    },

    remove: <T extends Table>(table: T, where: Partial<RowOf<T>>) => {
      const w = eqWhere(where as Record<string, unknown>);
      const result = db.query(`DELETE FROM ${table.name}${w.sql}`).run(...(w.params as never[]));
      return Number(result.changes ?? 0);
    },

    close: () => db.close(),
  };
};
