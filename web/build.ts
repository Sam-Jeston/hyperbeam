// Production build: bundles index.html with Tailwind into dist/, which the
// server embeds. `bun run watch` rebuilds on change.
import tailwind from "bun-plugin-tailwind";
import { rm } from "node:fs/promises";
import { copyFonts } from "../brand/build";

const watch = process.argv.includes("--watch");

async function build() {
  await rm("dist", { recursive: true, force: true });
  const result = await Bun.build({
    entrypoints: ["./index.html"],
    outdir: "dist",
    minify: true,
    publicPath: "/",
    plugins: [tailwind],
    // Fonts are copied into dist/fonts by copyFonts below.
    external: ["/fonts/*"],
    define: { "process.env.NODE_ENV": JSON.stringify("production") },
  });
  for (const log of result.logs) console.error(log);
  if (!result.success) process.exit(1);
  await copyFonts(import.meta.dir, "dist");
  console.log(`built ${result.outputs.length} files`);
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
