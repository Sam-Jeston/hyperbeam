// Serves the built landing page and docs (one directory: the site at /, the docs
// at /docs) as static files. Production counterpart of site/serve.ts, without
// the rebuild-on-change. scripts/site.sh deploys it with the files.
//
//   PUBLIC_DIR=./public PORT=4330 HOST=127.0.0.1 bun serve.ts
//
// Plain HTTP; something in front of it (cloudflared, a proxy) terminates TLS.

import { join, normalize, resolve } from "node:path";

const root = resolve(process.env.PUBLIC_DIR ?? join(import.meta.dir, "public"));
const port = Number(process.env.PORT ?? 4330);
const hostname = process.env.HOST ?? "127.0.0.1";
/** Requests for any other host are redirected here, e.g. www → apex. Unset: serve every host. */
const canonical = process.env.CANONICAL_HOST || null;

async function find(path: string) {
  for (const candidate of [path, join(path, "index.html"), `${path}.html`]) {
    const full = normalize(join(root, candidate));
    if (full !== root && !full.startsWith(root + "/")) return null;
    const f = Bun.file(full);
    if ((await f.exists()) && !full.endsWith("/")) return { file: f, full };
  }
  return null;
}

/** Hashed bundles and fonts never change under the same name; pages do. */
function cacheControl(full: string) {
  if (/\/assets\/[^/]+-[a-z0-9]{6,}\.(js|css)$/.test(full) || full.includes("/fonts/")) {
    return "public, max-age=31536000, immutable";
  }
  return "public, max-age=0, must-revalidate";
}

const securityHeaders = {
  "x-content-type-options": "nosniff",
  "referrer-policy": "strict-origin-when-cross-origin",
  "x-frame-options": "DENY",
};

Bun.serve({
  port,
  hostname,
  async fetch(req) {
    const url = new URL(req.url);
    const host = req.headers.get("host")?.split(":")[0];
    if (canonical && host && host !== canonical && host !== "127.0.0.1" && host !== "localhost") {
      return Response.redirect(`https://${canonical}${url.pathname}${url.search}`, 301);
    }
    if (req.method !== "GET" && req.method !== "HEAD") {
      return new Response("Method not allowed", { status: 405, headers: { allow: "GET, HEAD" } });
    }
    let path: string;
    try {
      path = decodeURIComponent(url.pathname);
    } catch {
      return new Response("Bad request", { status: 400 });
    }
    // One canonical form for directories: /docs/ → /docs, so relative links behave the same.
    if (path.length > 1 && path.endsWith("/")) {
      // Relative, so it keeps the scheme the visitor used (TLS ends in front of this server).
      return new Response(null, { status: 301, headers: { location: `${encodeURI(path.replace(/\/+$/, ""))}${url.search}` } });
    }
    const found = await find(path);
    if (found) {
      return new Response(req.method === "HEAD" ? null : found.file, {
        headers: { "cache-control": cacheControl(found.full), "content-type": found.file.type, ...securityHeaders },
      });
    }
    const notFound = Bun.file(join(root, "docs/404.html"));
    return new Response((await notFound.exists()) ? notFound : "Not found", {
      status: 404,
      headers: { "content-type": "text/html; charset=utf-8", "cache-control": "no-cache", ...securityHeaders },
    });
  },
});
console.log(`serving ${root} on http://${hostname}:${port}`);
