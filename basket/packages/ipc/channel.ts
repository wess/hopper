export type Channel<I, O> = {
  readonly name: string;
  readonly __i?: I;
  readonly __o?: O;
};

export type Event<T> = {
  readonly name: string;
  readonly __t?: T;
};

export const defineChannel = <I, O>(name: string): Channel<I, O> => ({ name });

export const defineEvent = <T>(name: string): Event<T> => ({ name });
