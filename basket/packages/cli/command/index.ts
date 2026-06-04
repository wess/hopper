export type FlagDef = {
  readonly short?: string;
  readonly type: "string" | "number" | "boolean";
  readonly default?: unknown;
  readonly description?: string;
};

export type CommandDef = {
  readonly name: string;
  readonly description?: string;
  readonly flags?: Record<string, FlagDef>;
  readonly args?: string[];
  readonly run: (parsed: ParsedArgs) => void | Promise<void>;
  readonly subcommands?: CommandDef[];
};

export type ParsedArgs = {
  readonly args: string[];
  readonly flags: Record<string, unknown>;
};

export const flag = (short: string, opts: Omit<FlagDef, "short">): FlagDef => ({ short, ...opts });

export const command = (name: string, opts: Omit<CommandDef, "name">): CommandDef => ({ name, ...opts });

export const parseArgs = (argv: string[], flagDefs: Record<string, FlagDef>): ParsedArgs => {
  const flags: Record<string, unknown> = {};
  const args: string[] = [];

  for (const [name, def] of Object.entries(flagDefs)) {
    if (def.default !== undefined) flags[name] = def.default;
  }

  const longMap = new Map<string, string>();
  const shortMap = new Map<string, string>();
  for (const [name, def] of Object.entries(flagDefs)) {
    longMap.set(`--${name}`, name);
    if (def.short) shortMap.set(`-${def.short}`, name);
  }

  let i = 0;
  while (i < argv.length) {
    const arg = argv[i]!;
    const flagName = longMap.get(arg) ?? shortMap.get(arg);

    if (flagName) {
      const def = flagDefs[flagName]!;
      if (def.type === "boolean") {
        flags[flagName] = true;
      } else {
        i++;
        const val = argv[i];
        flags[flagName] = def.type === "number" ? Number(val) : val;
      }
    } else {
      args.push(arg);
    }
    i++;
  }

  return { args, flags };
};

const printHelp = (name: string, commands: CommandDef[]): void => {
  console.log(`\nUsage: ${name} <command> [options]\n`);
  console.log("Commands:");
  for (const cmd of commands) {
    console.log(`  ${cmd.name.padEnd(16)} ${cmd.description ?? ""}`);
  }
  console.log();
};

export const cli = (name: string, commands: CommandDef[]): void => {
  const argv = process.argv.slice(2);

  if (argv.length === 0 || argv[0] === "help" || argv[0] === "--help" || argv[0] === "-h") {
    printHelp(name, commands);
    return;
  }

  const cmdName = argv[0];
  const cmd = commands.find((c) => c.name === cmdName);
  if (!cmd) {
    console.error(`Unknown command: ${cmdName}`);
    printHelp(name, commands);
    process.exit(1);
  }

  if (cmd.subcommands && argv.length > 1) {
    const subName = argv[1];
    const sub = cmd.subcommands.find((s) => s.name === subName);
    if (sub) {
      const parsed = parseArgs(argv.slice(2), sub.flags ?? {});
      void sub.run(parsed);
      return;
    }
  }

  const parsed = parseArgs(argv.slice(1), cmd.flags ?? {});
  void cmd.run(parsed);
};
