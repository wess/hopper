import { command } from "../command/index.ts";
import { scaffold } from "./scaffold.ts";

export const initCommand = command("init", {
  description: "Scaffold a new basket app from a template",
  flags: {
    template: { short: "t", type: "string", default: "minimal", description: "Template to use (minimal, menubar)" },
    name: { short: "n", type: "string", description: "Project name (defaults to first positional arg)" },
  },
  run: async (parsed) => {
    const name = (parsed.flags.name as string | undefined) ?? parsed.args[0];
    if (!name) {
      console.error("Usage: basket init <name> [--template <template>]");
      process.exit(1);
    }
    const template = (parsed.flags.template as string | undefined) ?? "minimal";
    await scaffold({ name, template });
  },
});
