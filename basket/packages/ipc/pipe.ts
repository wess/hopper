export type Ctx<I, A extends Record<string, unknown> = Record<string, unknown>> = {
  readonly input: I;
  readonly assigns: Readonly<A>;
};

export type Pipe<I, A extends Record<string, unknown> = Record<string, unknown>> = (
  ctx: Ctx<I, A>,
) => Ctx<I, A> | Promise<Ctx<I, A>>;

export type Validator<T> = { readonly parse: (value: unknown) => T };

export const assign = <A extends Record<string, unknown>, K extends string, V>(
  ctx: Ctx<unknown, A>,
  key: K,
  value: V,
): Ctx<unknown, A & Record<K, V>> => ({
  ...ctx,
  assigns: { ...ctx.assigns, [key]: value } as A & Record<K, V>,
});

export const validate =
  <I>(schema: Validator<I>): Pipe<I> =>
  (ctx) => ({
    ...ctx,
    input: schema.parse(ctx.input),
  });

export const pipeline =
  <I>(...pipes: readonly Pipe<I>[]) =>
  <O>(handler: (ctx: Ctx<I>) => O | Promise<O>) =>
  async (input: I): Promise<O> => {
    let ctx: Ctx<I> = { input, assigns: {} };
    for (const p of pipes) ctx = await p(ctx);
    return handler(ctx);
  };
