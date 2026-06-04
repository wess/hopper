import { command } from "../command/index.ts";

// docs/index.ts → packages/cli/docs/index.ts; 3 levels up resolves to basket/.
const ROOT = new URL("../../..", import.meta.url).pathname;

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
  "cli",
];
const KNOWN_DOCS = ["api", "cookbook", "overview", "quickstart"];

export const docsCommand = command("docs", {
  description: "Print package or doc reference to stdout",
  run: async (parsed) => {
    const target = parsed.args[0];
    if (!target) {
      console.log("Available packages:");
      for (const p of KNOWN_PACKAGES) console.log(`  basket docs ${p}`);
      console.log("\nAvailable docs:");
      for (const d of KNOWN_DOCS) console.log(`  basket docs ${d}`);
      return;
    }

    const candidates = [`${ROOT}packages/${target}/AGENTS.md`, `${ROOT}docs/${target}.md`];

    for (const path of candidates) {
      const f = Bun.file(path);
      if (await f.exists()) {
        process.stdout.write(await f.text());
        return;
      }
    }

    console.error(`No docs found for "${target}"`);
    console.error(`Looked in: ${candidates.join(", ")}`);
    process.exit(1);
  },
});
