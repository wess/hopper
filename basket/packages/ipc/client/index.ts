import type { Channel, Event } from "../channel.ts";
import { decode } from "../error.ts";

export type IpcInvocationError = Error & { readonly code: string; readonly data?: unknown };

export const isIpcInvocationError = (e: unknown): e is IpcInvocationError =>
  e instanceof Error && typeof (e as { code?: unknown }).code === "string";

const rethrow = (e: unknown): never => {
  const shape = decode(e);
  if (shape) {
    const err = new Error(shape.message) as IpcInvocationError;
    (err as { code: string }).code = shape.code;
    (err as { data?: unknown }).data = shape.data;
    throw err;
  }
  throw e;
};

export const invoke = async <I, O>(channel: Channel<I, O>, input: I, opts?: { timeout?: number }): Promise<O> => {
  try {
    return (await butter.invoke(channel.name, input, opts)) as O;
  } catch (e) {
    rethrow(e);
  }
  throw new Error("unreachable");
};

export const subscribe = <T>(event: Event<T>, fn: (data: T) => void): (() => void) => {
  const handler = (data: unknown) => fn(data as T);
  butter.on(event.name, handler);
  return () => butter.off(event.name, handler);
};

export const broadcast = <T>(event: Event<T>, data: T): void => {
  void butter.invoke(event.name, data);
};
