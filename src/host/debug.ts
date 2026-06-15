// Opt-in debug logging for the host. Off by default; many host operations are
// best-effort and swallow errors to keep the UI smooth (an exec resize that
// races a closing socket, a missing credential helper, a compose probe). That's
// the right UX but makes failures invisible — route those catch paths through
// `debug()` so they're diagnosable when HOPPER_DEBUG is set, without ever
// surfacing to the user.
//
// HOPPER_DEBUG="1" (or "true") enables all scopes; a comma-separated list
// (e.g. "exec,compose") enables only those. Output goes to stderr.

const parse = (v: string | undefined): { on: boolean; scopes: ReadonlySet<string> } => {
  if (!v || v === "0" || v.toLowerCase() === "false") return { on: false, scopes: new Set() };
  const scopes = new Set(
    v
      .split(",")
      .map((s) => s.trim())
      .filter((s) => s && s !== "1" && s.toLowerCase() !== "true"),
  );
  return { on: true, scopes };
};

const { on, scopes } = parse(process.env.HOPPER_DEBUG);

export const debug = (scope: string, ...args: unknown[]): void => {
  if (!on) return;
  if (scopes.size > 0 && !scopes.has(scope)) return;
  console.error(`[hopper:${scope}]`, ...args);
};
