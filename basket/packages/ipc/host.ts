import { on as butterOn, send as butterSend } from "butter";
import type { Channel, Event } from "./channel.ts";
import { encode } from "./error.ts";
import type { Validator } from "./pipe.ts";

export type HandleOptions<I> = {
  readonly input?: Validator<I>;
};

export const handle = <I, O>(
  channel: Channel<I, O>,
  handler: (input: I) => O | Promise<O>,
  options?: HandleOptions<I>,
): void => {
  butterOn(channel.name, async (raw) => {
    try {
      const input = options?.input ? options.input.parse(raw) : (raw as I);
      return await handler(input);
    } catch (e) {
      throw encode(e);
    }
  });
};

export const emit = <T>(event: Event<T>, data: T): void => {
  butterSend(event.name, data);
};

export const listen = <T>(event: Event<T>, fn: (data: T) => void): void => {
  butterOn(event.name, (data) => {
    fn(data as T);
  });
};
