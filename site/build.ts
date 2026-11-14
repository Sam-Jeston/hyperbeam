// Builds the landing page into dist/: React rendered to static HTML, one
// stylesheet, one small script, and the brand fonts.

import tailwind from "bun-plugin-tailwind";
import { mkdir, rm, writeFile } from "node:fs/promises";
import { basename } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { copyFonts, FAVICON, THEME_SCRIPT } from "../brand/build";

export async function build() {
  await rm("dist", { recursive: true, force: true });
  const assets = await Bun.build({
    entrypoints: ["src/client.ts", "src/styles.css"],
    outdir: "dist/assets",
    naming: "[name]-[hash].[ext]",
    minify: true,
    plugins: [tailwind],
    external: ["/fonts/*"],
  });
  for (const log of assets.logs) console.error(log);
  if (!assets.success) throw new Error("asset build failed");
  const href = (ext: string) => `/assets/${basename(assets.outputs.find((o) => o.path.endsWith(ext))!.path)}`;

  // Import fresh each build so `bun run dev` picks up edits.
  const { Page } = await import(`./src/page.tsx?${Date.now()}`);
  const body = renderToStaticMarkup(Page());
  const title = "Hyperbeam · Your team’s coding agents, working in the open";
  const description = "A self-hosted platform where a team’s coding agents work in the open. Start agents from the board, the CLI, Slack or Discord; each runs in its own sandbox with only its owner's permissions, and never holds your GitHub or tool credentials.";
  const html = `<!doctype html>
<html lang="en" class="dark">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${title}</title>
<meta name="description" content="${description}">
<meta property="og:title" content="${title}">
<meta property="og:description" content="${description}">
<meta name="theme-color" content="#0f1411">
<link rel="icon" href="${FAVICON}">
<link rel="preload" href="/fonts/bricolage-grotesque-latin-wght-normal.woff2" as="font" type="font/woff2" crossorigin>
<link rel="preload" href="/fonts/figtree-latin-wght-normal.woff2" as="font" type="font/woff2" crossorigin>
<script>${THEME_SCRIPT}</script>
<link rel="stylesheet" href="${href(".css")}">
</head>
<body>
${body}
<script type="module" src="${href(".js")}"></script>
</body>
</html>
`;
  await mkdir("dist", { recursive: true });
  await writeFile("dist/index.html", html);
  await copyFonts(import.meta.dir, "dist");
  console.log("built site → dist/");
}

if (import.meta.main) await build();
