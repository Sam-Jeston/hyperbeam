// Local preview: rebuilds on change and serves dist/ at http://localhost:4322/docs.

import { watch } from "node:fs";
import { join } from "node:path";
import { build } from "./build";
import { BASE } from "./src/config";

const port = Number(process.env.PORT ?? 4322);
const prefix = BASE === "/" ? "" : BASE;

await build();
let timer: ReturnType<typeof setTimeout> | null = null;
for (const dir of ["src", "content"]) {
  watch(dir, { recursive: true }, () => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => void build().catch((e) => console.error(e.message)), 150);
  });
}

Bun.serve({
  port,
  async fetch(req) {
    const path = decodeURIComponent(new URL(req.url).pathname);
    if (path === "/" && prefix) return Response.redirect(`${prefix}/`, 302);
    if (!path.startsWith(prefix)) return new Response("Not found", { status: 404 });
    const rel = path.slice(prefix.length) || "/";
    for (const candidate of [rel, join(rel, "index.html")]) {
      const f = Bun.file(join("dist", candidate));
      if (await f.exists() && !candidate.endsWith("/")) return new Response(f);
    }
    return new Response(Bun.file("dist/404.html"), { status: 404, headers: { "content-type": "text/html" } });
  },
});
console.log(`docs → http://localhost:${port}${prefix}/`);
