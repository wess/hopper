import { command } from "../command/index.ts";

const KNOWN_PACKAGES = [
  "config",
  "store",
  "ipc",
  "window",
  "menu",
  "tray",
  "db",
  "migrate",
  "request",
  "secrets",
  "auth",
  "dialog",
  "notify",
  "shortcut",
  "update",
  "protocol",
  "fs",
  "logger",
  "lifecycle",
  "cache",
  "theme",
  "ai",
  "mcp",
  "ui",
] as const;

export const addCommand = command("add", {
  description: "Add @basket/* packages to the current project's package.json",
  run: async (parsed) => {
    const requested = parsed.args;
    if (requested.length === 0) {
      console.error(`Usage: basket add <pkg> [<pkg>...]\n\nAvailable: ${KNOWN_PACKAGES.join(", ")}`);
      process.exit(1);
    }

    const cwd = process.cwd();
    const pkgPath = `${cwd}/package.json`;
    const pkgFile = Bun.file(pkgPath);
    if (!(await pkgFile.exists())) {
      console.error(`No package.json found at ${pkgPath}`);
      process.exit(1);
    }

    const pkg = (await pkgFile.json()) as { dependencies?: Record<string, string> };
    const deps = { ...(pkg.dependencies ?? {}) };

    for (const name of requested) {
      const stripped = name.replace(/^@basket\//, "");
      if (!KNOWN_PACKAGES.includes(stripped as (typeof KNOWN_PACKAGES)[number])) {
        console.error(`Unknown package: ${name}\nAvailable: ${KNOWN_PACKAGES.join(", ")}`);
        process.exit(1);
      }
      deps[`@basket/${stripped}`] = "workspace:*";
    }

    const next = { ...pkg, dependencies: deps };
    await Bun.write(pkgPath, `${JSON.stringify(next, null, 2)}\n`);
    console.log(`Added ${requested.length} package(s). Run \`bun install\`.`);
  },
});
