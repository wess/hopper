export type IpcErrorShape = {
  readonly code: string;
  readonly message: string;
  readonly data?: unknown;
};

const TAG = "__basketIpcError__";

const tag = (shape: IpcErrorShape): Error => {
  const err = new Error(shape.message);
  Object.assign(err, { [TAG]: shape });
  return err;
};

const taggedShape = (e: Error): IpcErrorShape | undefined =>
  (e as unknown as Record<string, unknown>)[TAG] as IpcErrorShape | undefined;

export const ipcError = (code: string, message: string, data?: unknown): Error => tag({ code, message, data });

export const notFound = (what: string, data?: unknown) => ipcError("not_found", `${what} not found`, data);
export const unauthorized = (message = "Unauthorized", data?: unknown) => ipcError("unauthorized", message, data);
export const forbidden = (message = "Forbidden", data?: unknown) => ipcError("forbidden", message, data);
export const conflict = (message: string, data?: unknown) => ipcError("conflict", message, data);
export const invalidInput = (message: string, data?: unknown) => ipcError("invalid_input", message, data);
export const internal = (message = "Internal error", data?: unknown) => ipcError("internal", message, data);

export const encode = (e: unknown): Error => {
  if (e instanceof Error) {
    const shape = taggedShape(e);
    if (shape) return new Error(`__ipc__:${JSON.stringify(shape)}`);
    return new Error(`__ipc__:${JSON.stringify({ code: "internal", message: e.message })}`);
  }
  return new Error(`__ipc__:${JSON.stringify({ code: "internal", message: String(e) })}`);
};

export const decode = (e: unknown): { code: string; message: string; data?: unknown } | undefined => {
  if (!(e instanceof Error)) return undefined;
  if (!e.message.startsWith("__ipc__:")) return undefined;
  try {
    return JSON.parse(e.message.slice("__ipc__:".length));
  } catch {
    return undefined;
  }
};
