import { mkdir, readdir } from "node:fs/promises";
import { dirname, join, relative } from "node:path";

export type ScaffoldOptions = {
  readonly name: string;
  readonly template: string;
  readonly cwd?: string;
};

const templatesRoot = (): string => {
  // scaffold.ts → packages/cli/init/; go up 3 dirs to basket/, then templates/
  const here = new URL(".", import.meta.url).pathname;
  return join(here, "..", "..", "..", "templates");
};

const substitute = (content: string, vars: Record<string, string>): string => {
  return content.replace(/\{\{(\w+)\}\}/g, (_, key) => vars[key] ?? `{{${key}}}`);
};

const isBinary = (path: string): boolean => /\.(png|jpg|jpeg|gif|ico|icns|webp|woff2?|ttf|otf|zip)$/i.test(path);

const walk = async (dir: string): Promise<string[]> => {
  const out: string[] = [];
  const entries = await readdir(dir, { withFileTypes: true });
  for (const e of entries) {
    const full = join(dir, e.name);
    if (e.isDirectory()) out.push(...(await walk(full)));
    else out.push(full);
  }
  return out;
};

export const scaffold = async (opts: ScaffoldOptions): Promise<void> => {
  const cwd = opts.cwd ?? process.cwd();
  const target = join(cwd, opts.name);
  const src = join(templatesRoot(), opts.template);

  const srcExists = await Bun.file(join(src, "package.json.tmpl")).exists();
  if (!srcExists) {
    console.error(`Template "${opts.template}" not found at ${src}`);
    process.exit(1);
  }

  const targetExists = await Bun.file(target).exists();
  if (targetExists) {
    console.error(`Target directory already exists: ${target}`);
    process.exit(1);
  }

  console.log(`Scaffolding ${opts.template} → ${target}`);

  const files = await walk(src);
  const vars = { name: opts.name };

  for (const file of files) {
    const rel = relative(src, file);
    let outRel = rel.replace(/\.tmpl$/, "");
    outRel = substitute(outRel, vars);
    const outPath = join(target, outRel);
    await mkdir(dirname(outPath), { recursive: true });

    if (isBinary(file)) {
      const bytes = await Bun.file(file).arrayBuffer();
      await Bun.write(outPath, bytes);
    } else {
      const content = await Bun.file(file).text();
      await Bun.write(outPath, substitute(content, vars));
    }
  }

  console.log(`\nCreated ${opts.name}/. Next:\n`);
  console.log(`  cd ${opts.name}`);
  console.log(`  bun install`);
  console.log(`  bun run dev\n`);
};
