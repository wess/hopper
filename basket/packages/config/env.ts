type EnvOptions<T> = {
  parse?: (value: string) => T;
  default?: string;
};

type EnvRef<T> = {
  readonly read: () => T;
};

export const env = <T = string>(name: string, options?: EnvOptions<T>): EnvRef<T> => ({
  read: () => {
    const raw = Bun.env[name] ?? options?.default;
    if (raw === undefined) {
      throw new Error(
        `Missing required environment variable: ${name}. Set it in your .env file or export it in your shell.`,
      );
    }
    if (options?.parse) {
      return options.parse(raw);
    }
    return raw as unknown as T;
  },
});
