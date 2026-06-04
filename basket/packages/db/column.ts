export type ColumnType = "text" | "integer" | "real" | "blob" | "boolean" | "timestamp" | "serial";

export type ColData<T> = {
  readonly type: ColumnType;
  readonly isPrimaryKey?: true;
  readonly isUnique?: true;
  readonly isNullable?: true;
  readonly defaultValue?: string | number | boolean | null;
  readonly __t?: T;
};

export type Col<T> = ColData<T> & {
  readonly primaryKey: () => Col<T>;
  readonly unique: () => Col<T>;
  readonly nullable: () => Col<T | null>;
  readonly default: (v: string | number | boolean | null) => Col<T>;
};

const wrap = <T>(data: ColData<T>): Col<T> => ({
  ...data,
  primaryKey: () => wrap({ ...data, isPrimaryKey: true }),
  unique: () => wrap({ ...data, isUnique: true }),
  nullable: () => wrap<T | null>({ ...data, isNullable: true }),
  default: (v) => wrap({ ...data, defaultValue: v }),
});

export const column = {
  text: (): Col<string> => wrap<string>({ type: "text" }),
  integer: (): Col<number> => wrap<number>({ type: "integer" }),
  real: (): Col<number> => wrap<number>({ type: "real" }),
  blob: (): Col<Uint8Array> => wrap<Uint8Array>({ type: "blob" }),
  boolean: (): Col<boolean> => wrap<boolean>({ type: "boolean" }),
  timestamp: (): Col<string> => wrap<string>({ type: "timestamp" }),
  serial: (): Col<number> => wrap<number>({ type: "serial" }),
};
