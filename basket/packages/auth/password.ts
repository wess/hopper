export const hashPassword = (password: string): Promise<string> =>
  Bun.password.hash(password, { algorithm: "argon2id" });

export const verifyPassword = (password: string, hash: string): Promise<boolean> => Bun.password.verify(password, hash);
