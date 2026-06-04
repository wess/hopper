type EnvRef<T> = { readonly read: () => T };

const isEnvRef = (value: unknown): value is EnvRef<unknown> =>
  typeof value === "object" && value !== null && "read" in value && typeof (value as any).read === "function";

type ResolveConfig<T> = {
  [K in keyof T]: T[K] extends EnvRef<infer U> ? U : T[K] extends Record<string, unknown> ? ResolveConfig<T[K]> : T[K];
};

const resolve = (obj: Record<string, unknown>): Record<string, unknown> => {
  const result: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(obj)) {
    if (isEnvRef(value)) {
      result[key] = value.read();
    } else if (typeof value === "object" && value !== null && !Array.isArray(value)) {
      result[key] = Object.freeze(resolve(value as Record<string, unknown>));
    } else {
      result[key] = value;
    }
  }
  return result;
};

export const defineConfig = <T extends Record<string, unknown>>(schema: T): ResolveConfig<T> => {
  return Object.freeze(resolve(schema)) as ResolveConfig<T>;
};
