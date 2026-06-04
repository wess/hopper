import type { RowOf, Table } from "./table.ts";

type WhereOp = "=" | "!=" | "<" | "<=" | ">" | ">=" | "LIKE" | "IN" | "IS NULL" | "IS NOT NULL";

type WhereClause = {
  readonly col: string;
  readonly op: WhereOp;
  readonly value?: unknown;
};

export type Query<R> = {
  readonly table: string;
  readonly columns: readonly string[];
  readonly wheres: readonly WhereClause[];
  readonly orderBy?: { col: string; dir: "asc" | "desc" };
  readonly limitN?: number;
  readonly offsetN?: number;
  readonly __row?: R;
};

type ColRef<R> = {
  equals: (v: unknown) => WhereClause;
  notEquals: (v: unknown) => WhereClause;
  lt: (v: unknown) => WhereClause;
  lte: (v: unknown) => WhereClause;
  gt: (v: unknown) => WhereClause;
  gte: (v: unknown) => WhereClause;
  like: (v: string) => WhereClause;
  in: (vs: readonly unknown[]) => WhereClause;
  isNull: () => WhereClause;
  isNotNull: () => WhereClause;
  __r?: R;
};

const colRef = <R>(name: string): ColRef<R> => ({
  equals: (value) => ({ col: name, op: "=", value }),
  notEquals: (value) => ({ col: name, op: "!=", value }),
  lt: (value) => ({ col: name, op: "<", value }),
  lte: (value) => ({ col: name, op: "<=", value }),
  gt: (value) => ({ col: name, op: ">", value }),
  gte: (value) => ({ col: name, op: ">=", value }),
  like: (value) => ({ col: name, op: "LIKE", value }),
  in: (vs) => ({ col: name, op: "IN", value: vs }),
  isNull: () => ({ col: name, op: "IS NULL" }),
  isNotNull: () => ({ col: name, op: "IS NOT NULL" }),
});

type Q<R> = (col: keyof R & string) => ColRef<R>;

export type Buildable<R> = Query<R> & {
  readonly where: (fn: (q: Q<R>) => WhereClause) => Buildable<R>;
  readonly select: <K extends keyof R & string>(...cols: K[]) => Buildable<Pick<R, K>>;
  readonly order: (col: keyof R & string, dir?: "asc" | "desc") => Buildable<R>;
  readonly limit: (n: number) => Buildable<R>;
  readonly offset: (n: number) => Buildable<R>;
};

const build = <R>(q: Query<R>): Buildable<R> => ({
  ...q,
  where: (fn) => build({ ...q, wheres: [...q.wheres, fn(colRef as Q<R>)] }),
  select: <K extends keyof R & string>(...cols: K[]) =>
    build<Pick<R, K>>({ ...q, columns: cols }) as Buildable<Pick<R, K>>,
  order: (col, dir = "asc") => build({ ...q, orderBy: { col: col as string, dir } }),
  limit: (n) => build({ ...q, limitN: n }),
  offset: (n) => build({ ...q, offsetN: n }),
});

export const from = <T extends Table>(table: T): Buildable<RowOf<T>> =>
  build<RowOf<T>>({
    table: table.name,
    columns: [],
    wheres: [],
  });

export const renderQuery = (q: Query<unknown>): { sql: string; params: unknown[] } => {
  const cols = q.columns.length === 0 ? "*" : q.columns.join(", ");
  const params: unknown[] = [];
  let sql = `SELECT ${cols} FROM ${q.table}`;

  if (q.wheres.length > 0) {
    const parts = q.wheres.map((w) => {
      if (w.op === "IS NULL" || w.op === "IS NOT NULL") return `${w.col} ${w.op}`;
      if (w.op === "IN") {
        const vs = w.value as unknown[];
        const placeholders = vs.map(() => "?").join(", ");
        params.push(...vs);
        return `${w.col} IN (${placeholders})`;
      }
      params.push(w.value);
      return `${w.col} ${w.op} ?`;
    });
    sql += ` WHERE ${parts.join(" AND ")}`;
  }

  if (q.orderBy) sql += ` ORDER BY ${q.orderBy.col} ${q.orderBy.dir.toUpperCase()}`;
  if (q.limitN !== undefined) sql += ` LIMIT ${q.limitN}`;
  if (q.offsetN !== undefined) sql += ` OFFSET ${q.offsetN}`;

  return { sql, params };
};
