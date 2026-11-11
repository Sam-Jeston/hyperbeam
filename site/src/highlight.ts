// Build-time code highlighting. Colors come from the --shiki-* tokens in brand/tokens.css,
// so code follows the light and dark themes.

import { createCssVariablesTheme, createHighlighter } from "shiki";

const theme = createCssVariablesTheme({ name: "hyperbeam", variablePrefix: "--shiki-", fontStyle: true });

const highlighter = await createHighlighter({ themes: [theme], langs: ["yaml", "bash", "sh", "json", "toml", "markdown", "rust", "ts", "text"] });

export function highlight(code: string, lang = "text"): string {
  const known = highlighter.getLoadedLanguages().includes(lang) ? lang : "text";
  return highlighter.codeToHtml(code.replace(/\n$/, ""), { lang: known, theme: "hyperbeam" });
}
