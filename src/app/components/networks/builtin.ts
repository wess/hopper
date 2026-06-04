// Built-in Docker networks that can't be removed or pruned.

const BUILTIN = new Set(["bridge", "host", "none"]);

export const isBuiltin = (name: string): boolean => BUILTIN.has(name);
