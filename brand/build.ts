// Build helpers shared by web/, site/ and docs/. Dependency-free on purpose:
// brand/ has no node_modules, so each app passes in what it needs.

import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { basename, join } from "node:path";

const TOKENS = join(import.meta.dir, "tokens.css");

/** Font files that tokens.css references as /fonts/<name>. */
export async function fontNames(): Promise<string[]> {
  const css = await readFile(TOKENS, "utf8");
  return [...new Set([...css.matchAll(/url\("\/fonts\/([^"]+)"\)/g)].map((m) => m[1]))];
}

/**
 * Copy the fonts tokens.css uses from an app's @fontsource-variable packages
 * into `<outDir>/fonts`. They are copied, not inlined, so the stylesheet stays
 * small and each face is cached on its own.
 */
export async function copyFonts(appDir: string, outDir: string) {
  const wanted = new Set(await fontNames());
  await mkdir(join(outDir, "fonts"), { recursive: true });
  const glob = new Bun.Glob("node_modules/@fontsource-variable/*/files/*.woff2");
  for await (const path of glob.scan(appDir)) {
    const name = basename(path);
    if (wanted.delete(name)) await copyFile(join(appDir, path), join(outDir, "fonts", name));
  }
  if (wanted.size) throw new Error(`fonts not found under ${appDir}/node_modules: ${[...wanted].join(", ")}`);
}

/** Point a built stylesheet's /fonts/ URLs at a site served under `base` (e.g. "/docs"). */
export async function rebaseFonts(cssFile: string, base: string) {
  if (!base || base === "/") return;
  const css = await readFile(cssFile, "utf8");
  await writeFile(cssFile, css.replaceAll('url("/fonts/', `url("${base.replace(/\/$/, "")}/fonts/`).replaceAll("url(/fonts/", `url(${base.replace(/\/$/, "")}/fonts/`));
}

/** Inline script for <head>: apply the saved theme before first paint. Dark unless someone chose otherwise. */
export const THEME_SCRIPT = `(function(){var t="dark";try{t=localStorage.getItem("noemune:theme")||"dark"}catch(e){}var l=t==="light"||(t==="system"&&matchMedia("(prefers-color-scheme: light)").matches);document.documentElement.className=l?"light":"dark"})()`;

/** The mark as a data: URL, for favicons. */
export const FAVICON =
  "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'%3E%3Crect width='32' height='32' rx='7' fill='%23161d19'/%3E%3Ccircle cx='16' cy='16' r='11.5' fill='none' stroke='%23b98ae0' stroke-width='2.6'/%3E%3Ccircle cx='12.4' cy='16.8' r='3.9' fill='%23b98ae0'/%3E%3Ccircle cx='20.3' cy='11.4' r='2.5' fill='%23ee7c9c'/%3E%3Ccircle cx='20.5' cy='20.9' r='2.5' fill='%23ee7c9c'/%3E%3C/svg%3E";
