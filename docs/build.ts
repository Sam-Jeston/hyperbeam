// Builds the docs into dist/: one static HTML page per Markdown file in content/,
// a search index, llms.txt, the stylesheet, a small script and the brand fonts.
// dist/ is meant to be served at BASE (default /docs).

import tailwind from "bun-plugin-tailwind";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { basename, dirname, join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { copyFonts, FAVICON, rebaseFonts, THEME_SCRIPT } from "../brand/build";

export async function build() {
  // Fresh imports so `bun run dev` picks up edits to the layout and content.
  const v = Date.now();
  const { BASE } = await import(`./src/config.ts?${v}`);
  const { frontMatter, render, href } = await import(`./src/markdown.ts?${v}`);
  const { NAV, ORDER } = await import(`./src/nav.ts?${v}`);
  const { DocPage } = await import(`./src/layout.tsx?${v}`);
  const { Home } = await import(`./src/home.tsx?${v}`);

  await rm("dist", { recursive: true, force: true });

  // Content.
  const pages: Record<string, any> = {};
  for (const slug of ORDER) {
    const source = await readFile(join("content", `${slug}.md`), "utf8").catch(() => {
      throw new Error(`nav.ts lists "${slug}" but content/${slug}.md does not exist`);
    });
    const { data, body } = frontMatter(source);
    if (!data.title) throw new Error(`content/${slug}.md has no title in its front matter`);
    pages[slug] = { slug, title: data.title, description: data.description ?? "", body, ...render(body) };
  }
  const titles = Object.fromEntries(Object.values(pages).map((p: any) => [p.slug, p.title]));
  const groupOf = (slug: string) => NAV.find((g: any) => g.pages.includes(slug))?.title;

  // Every internal link must land on a page (and a heading, if it names one).
  const broken: string[] = [];
  for (const page of Object.values(pages) as any[]) {
    for (const m of page.html.matchAll(/href="([^"]+)"/g)) {
      const link: string = m[1];
      const prefix = BASE === "/" ? "/" : `${BASE}/`;
      if (!link.startsWith(prefix) && link !== BASE) continue;
      const [path, hash] = link.slice(prefix.length).split("#");
      const target = path === "" ? null : pages[path];
      if (path !== "" && !target) broken.push(`${page.slug}: ${link}`);
      else if (hash && target && !target.headings.some((h: any) => h.id === hash)) broken.push(`${page.slug}: ${link} (no such heading)`);
    }
  }
  if (broken.length) throw new Error(`broken links:\n  ${broken.join("\n  ")}`);

  // Assets.
  const assets = await Bun.build({
    entrypoints: ["src/client.ts", "src/styles.css"],
    outdir: "dist/assets",
    naming: "[name]-[hash].[ext]",
    minify: true,
    plugins: [tailwind],
    external: ["/fonts/*"],
    define: { DOCS_BASE: JSON.stringify(BASE) },
  });
  for (const log of assets.logs) console.error(log);
  if (!assets.success) throw new Error("asset build failed");
  const out = (ext: string) => assets.outputs.find((o) => o.path.endsWith(ext))!.path;
  await rebaseFonts(out(".css"), BASE);
  const asset = (ext: string) => href(`/assets/${basename(out(ext))}`);
  const font = (name: string) => href(`/fonts/${name}`);

  const document = (title: string, description: string, body: string) => `<!doctype html>
<html lang="en" class="dark">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${title}</title>
<meta name="description" content="${description.replace(/"/g, "&quot;")}">
<meta name="theme-color" content="#0f1411">
<link rel="icon" href="${FAVICON}">
<link rel="preload" href="${font("figtree-latin-wght-normal.woff2")}" as="font" type="font/woff2" crossorigin>
<script>${THEME_SCRIPT}</script>
<link rel="stylesheet" href="${asset(".css")}">
</head>
<body>
${body}
<script type="module" src="${asset(".js")}"></script>
</body>
</html>
`;
  const write = async (path: string, html: string) => {
    await mkdir(dirname(join("dist", path)), { recursive: true });
    await writeFile(join("dist", path), html);
  };

  // Home.
  const metas = Object.fromEntries(Object.values(pages).map((p: any) => [p.slug, { slug: p.slug, title: p.title, description: p.description }]));
  await write(
    "index.html",
    document(
      "Noemune docs",
      "Documentation for Noemune, the self-hosted platform where a team’s coding agents work in the open.",
      renderToStaticMarkup(
        DocPage({
          meta: { slug: "", title: "Noemune documentation", description: "Everything you need to run your team’s coding agents in the open: try it locally, learn how it fits together, and install it for your team." },
          headings: [],
          titles,
          next: metas[ORDER[0]],
          wide: true,
          children: Home({ pages: metas }),
        }),
      ),
    ),
  );

  // Pages.
  for (const [i, slug] of ORDER.entries()) {
    const page = pages[slug];
    const html = renderToStaticMarkup(
      DocPage({
        meta: metas[slug],
        group: groupOf(slug),
        html: page.html,
        headings: page.headings,
        titles,
        prev: i > 0 ? metas[ORDER[i - 1]] : undefined,
        next: i < ORDER.length - 1 ? metas[ORDER[i + 1]] : undefined,
        editPath: `${slug}.md`,
      }),
    );
    await write(`${slug}/index.html`, document(`${page.title} · Noemune docs`, page.description, html));
    await write(`${slug}.md`, `# ${page.title}\n\n${page.description ? `> ${page.description}\n\n` : ""}${page.body}`);
  }

  // Not found.
  await write(
    "404.html",
    document("Not found · Noemune docs", "", renderToStaticMarkup(
      DocPage({
        meta: { slug: "404", title: "This page isn’t here", description: "It may have moved. Search the docs, or start from the overview." },
        headings: [],
        titles,
        children: Home({ pages: metas }),
        wide: true,
      }),
    )),
  );

  // Search index and llms.txt.
  await write(
    "search-index.json",
    JSON.stringify(ORDER.map((slug: string) => ({ slug, title: pages[slug].title, description: pages[slug].description, group: groupOf(slug), sections: pages[slug].sections }))),
  );
  const llms = [
    "# Noemune",
    "",
    "> A self-hosted platform where a team's coding agents work in the open.",
    "",
    ...NAV.flatMap((g: any) => [`## ${g.title}`, "", ...g.pages.map((s: string) => `- [${pages[s].title}](${href(`/${s}.md`)}): ${pages[s].description}`), ""]),
  ].join("\n");
  await write("llms.txt", llms);

  await copyFonts(import.meta.dir, "dist");
  console.log(`built ${ORDER.length + 1} docs pages → dist/ (served at ${BASE})`);
}

if (import.meta.main) await build();
