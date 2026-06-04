import { deleteSecret, getSecret, setSecret } from "./keychain.ts";

export type Vault = {
  readonly get: (key: string) => Promise<string | undefined>;
  readonly set: (key: string, value: string) => Promise<void>;
  readonly delete: (key: string) => Promise<void>;
  readonly getJson: <T>(key: string) => Promise<T | undefined>;
  readonly setJson: (key: string, value: unknown) => Promise<void>;
};

export const createVault = (service: string): Vault => ({
  get: (key) => getSecret(service, key),
  set: (key, value) => setSecret(service, key, value),
  delete: (key) => deleteSecret(service, key),
  getJson: async <T>(key: string) => {
    const raw = await getSecret(service, key);
    if (raw === undefined) return undefined;
    try {
      return JSON.parse(raw) as T;
    } catch {
      return undefined;
    }
  },
  setJson: (key, value) => setSecret(service, key, JSON.stringify(value)),
});
