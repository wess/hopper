// Minimal static server for the Hopper marketing site.
//   bun run site/serve.ts          → http://localhost:3000
//   PORT=8080 bun run site/serve.ts
import { file } from "bun";
import { join } from "node:path";

const dir = import.meta.dir;
const port = Number(process.env.PORT ?? 3000);

const server = Bun.serve({
  port,
  fetch(req) {
    const url = new URL(req.url);
    const path = url.pathname === "/" ? "/index.html" : url.pathname;
    return new Response(file(join(dir, path)));
  },
});

console.log(`Hopper site → http://localhost:${server.port}`);
