import { createVault, type Vault } from "@basket/secrets";

export type Session<T> = {
  readonly get: () => Promise<T | undefined>;
  readonly set: (value: T) => Promise<void>;
  readonly clear: () => Promise<void>;
  readonly isSignedIn: () => Promise<boolean>;
};

export type SessionOptions = {
  readonly service: string;
  readonly key?: string;
  readonly vault?: Vault;
};

export const createSession = <T>(options: SessionOptions): Session<T> => {
  const vault = options.vault ?? createVault(options.service);
  const key = options.key ?? "session";

  return {
    get: () => vault.getJson<T>(key),
    set: (value) => vault.setJson(key, value),
    clear: () => vault.delete(key),
    isSignedIn: async () => (await vault.get(key)) !== undefined,
  };
};
