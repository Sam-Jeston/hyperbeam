// Production build: bundles index.html with Tailwind into dist/, which the
// server embeds. `bun run watch` rebuilds on change.
import tailwind from "bun-plugin-tailwind";
import { copyFile, mkdir, rm } from "node:fs/promises";
import { basename } from "node:path";

const watch = process.argv.includes("--watch");

async function build() {
  await rm("dist", { recursive: true, force: true });
  const result = await Bun.build({
    entrypoints: ["./index.html"],
    outdir: "dist",
    minify: true,
    publicPath: "/",
    plugins: [tailwind],
    // Fonts are copied into dist/fonts by copyFonts() below.
    external: ["/fonts/*"],
    define: { "process.env.NODE_ENV": JSON.stringify("production") },
  });
  for (const log of result.logs) console.error(log);
  if (!result.success) process.exit(1);
  await copyFonts();
  console.log(`built ${result.outputs.length} files`);
}

// The fonts app.css references by /fonts/… URL, copied rather than inlined so
// the stylesheet stays small and each face is cached on its own.
async function copyFonts() {
  const css = await Bun.file("src/app.css").text();
  const names = [...css.matchAll(/url\("\/fonts\/([^"]+)"\)/g)].map((m) => m[1]);
  await mkdir("dist/fonts", { recursive: true });
  const files = new Bun.Glob("node_modules/@fontsource-variable/*/files/*.woff2");
  const wanted = new Set(names);
  for await (const path of files.scan(".")) {
    if (wanted.delete(basename(path))) await copyFile(path, `dist/fonts/${basename(path)}`);
  }
  if (wanted.size) throw new Error(`fonts not found: ${[...wanted].join(", ")}`);
}

await build();
if (watch) {
  const { watch: fsWatch } = await import("node:fs");
  let timer: ReturnType<typeof setTimeout> | null = null;
  fsWatch("src", { recursive: true }, () => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => void build().catch(console.error), 120);
  });
  console.log("watching src/ …");
}
