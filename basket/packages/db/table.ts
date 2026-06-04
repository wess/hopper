import type { Col } from "./column.ts";

export type ColumnsMap = Record<string, Col<unknown>>;

export type Table<C extends ColumnsMap = ColumnsMap> = {
  readonly name: string;
  readonly columns: C;
};

export type RowOf<T> = T extends Table<infer C> ? { [K in keyof C]: C[K] extends Col<infer V> ? V : never } : never;

export const defineTable = <C extends ColumnsMap>(name: string, columns: C): Table<C> => ({ name, columns });
