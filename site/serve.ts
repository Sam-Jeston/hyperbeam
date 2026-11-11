// Local preview: rebuilds on change and serves dist/ on http://localhost:4321.
// If docs/ has been built, it is served under /docs, as it would be in production.

import { watch } from "node:fs";
import { join, resolve } from "node:path";
import { build } from "./build";

const port = Number(process.env.PORT ?? 4321);
const docsDist = resolve(import.meta.dir, "../docs/dist");

await build();
let timer: ReturnType<typeof setTimeout> | null = null;
watch("src", { recursive: true }, () => {
  if (timer) clearTimeout(timer);
  timer = setTimeout(() => void build().catch(console.error), 150);
});

async function file(root: string, path: string) {
  for (const candidate of [path, join(path, "index.html"), `${path}.html`]) {
    const f = Bun.file(join(root, candidate));
    if (await f.exists()) return f;
  }
  return null;
}

Bun.serve({
  port,
  async fetch(req) {
    const path = decodeURIComponent(new URL(req.url).pathname);
    const found = path === "/docs" || path.startsWith("/docs/") ? await file(docsDist, path.slice(5) || "/") : await file("dist", path);
    return found ? new Response(found) : new Response("Not found", { status: 404 });
  },
});
console.log(`site → http://localhost:${port}`);
