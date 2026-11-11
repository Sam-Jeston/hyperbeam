// Markdown → HTML for docs pages, at build time. Adds heading anchors, highlighted
// code with titles and copy buttons, callouts (> [!NOTE]), scrollable tables, and
// base-relative internal links. Also extracts the outline and searchable text.

import { Marked, type Token, type Tokens } from "marked";
import { BASE } from "./config";
import { highlight } from "./highlight";

export interface Heading {
  depth: 2 | 3;
  id: string;
  text: string;
}

export interface Section {
  heading: string;
  id: string;
  text: string;
}

export interface Rendered {
  html: string;
  headings: Heading[];
  sections: Section[];
}

export function slugify(text: string): string {
  return text
    .toLowerCase()
    .replace(/<[^>]+>/g, "")
    .replace(/[`*_~]/g, "")
    .replace(/[^a-z0-9\s-]/g, "")
    .trim()
    .replace(/\s+/g, "-");
}

const escape = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
const plain = (s: string) => s.replace(/<[^>]+>/g, "").replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&#39;/g, "'");

const CALLOUTS: Record<string, { label: string; icon: string }> = {
  NOTE: { label: "Note", icon: '<circle cx="12" cy="12" r="10"/><path d="M12 16v-4M12 8h.01"/>' },
  TIP: { label: "Tip", icon: '<path d="M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5M9 18h6M10 22h4"/>' },
  WARNING: { label: "Warning", icon: '<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3M12 9v4M12 17h.01"/>' },
  IMPORTANT: { label: "Important", icon: '<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/>' },
};

const COPY_ICON = '<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect width="14" height="14" x="8" y="8" rx="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>';
const LINK_ICON = '<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/></svg>';

/** Internal links are written from the docs root ("/quickstart"); serve them under the base. */
export function href(path: string): string {
  if (!path.startsWith("/") || path.startsWith("//")) return path;
  const base = BASE === "/" ? "" : BASE;
  return `${base}${path === "/" ? "/" : path}`;
}

export function render(markdown: string): Rendered {
  const headings: Heading[] = [];
  const used = new Map<string, number>();
  const uniqueId = (text: string) => {
    const id = slugify(text) || "section";
    const n = used.get(id) ?? 0;
    used.set(id, n + 1);
    return n ? `${id}-${n}` : id;
  };

  const marked = new Marked({ gfm: true });
  marked.use({
    renderer: {
      heading({ tokens, depth }: Tokens.Heading) {
        const inner = this.parser.parseInline(tokens);
        const text = plain(inner);
        const id = uniqueId(text);
        if (depth === 2 || depth === 3) headings.push({ depth, id, text });
        return `<h${depth} id="${id}"><a class="anchor" href="#${id}" aria-label="Link to this section">${LINK_ICON}</a>${inner}</h${depth}>\n`;
      },
      code({ text, lang }: Tokens.Code) {
        const info = (lang ?? "").trim();
        const language = info.split(/\s+/)[0] || "text";
        const title = /title="([^"]+)"/.exec(info)?.[1];
        const label = title ?? (language === "text" ? "" : language === "sh" || language === "bash" ? "Terminal" : language);
        return `<div class="code-block">${
          label ? `<div class="code-head"><span>${escape(label)}</span>` : `<div class="code-head code-head-bare"><span></span>`
        }<button type="button" class="copy" data-copy="${escape(text)}" aria-label="Copy code">${COPY_ICON}<span>Copy</span></button></div>${highlight(text, language)}</div>\n`;
      },
      blockquote({ tokens }: Tokens.Blockquote) {
        const body = this.parser.parse(tokens);
        const match = /^<p>\[!(NOTE|TIP|WARNING|IMPORTANT)\]\s*/.exec(body);
        if (!match) return `<blockquote>${body}</blockquote>\n`;
        const kind = match[1];
        const { label, icon } = CALLOUTS[kind];
        const rest = body.replace(match[0], "<p>").replace(/^<p>\s*<\/p>/, "");
        return `<div class="callout callout-${kind.toLowerCase()}"><div class="callout-title"><svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${icon}</svg>${label}</div>${rest}</div>\n`;
      },
      table(token: Tokens.Table) {
        const cell = (c: Tokens.TableCell) => this.parser.parseInline(c.tokens);
        const head = token.header.map((c) => `<th>${cell(c)}</th>`).join("");
        const rows = token.rows.map((r) => `<tr>${r.map((c) => `<td>${cell(c)}</td>`).join("")}</tr>`).join("");
        return `<div class="table-wrap"><table><thead><tr>${head}</tr></thead><tbody>${rows}</tbody></table></div>\n`;
      },
      link({ href: target, title, tokens }: Tokens.Link) {
        const inner = this.parser.parseInline(tokens);
        const external = /^https?:\/\//.test(target);
        return `<a href="${escape(href(target))}"${title ? ` title="${escape(title)}"` : ""}${external ? ' rel="noreferrer noopener" target="_blank"' : ""}>${inner}</a>`;
      },
    },
  });

  const tokens = marked.lexer(markdown);
  const html = marked.parser(tokens);
  return { html, headings, sections: sections(tokens, headings) };
}

/** Searchable text, split at each h2/h3 so results can link to the right place. */
function sections(tokens: Token[], headings: Heading[]): Section[] {
  const out: Section[] = [{ heading: "", id: "", text: "" }];
  let h = 0;
  for (const token of tokens) {
    if (token.type === "heading" && (token.depth === 2 || token.depth === 3)) {
      const heading = headings[h++];
      out.push({ heading: heading.text, id: heading.id, text: "" });
      continue;
    }
    if (token.type === "code" || token.type === "space" || token.type === "hr") continue;
    const text = plain("text" in token && typeof token.text === "string" ? token.text : token.raw)
      .replace(/\[!(NOTE|TIP|WARNING|IMPORTANT)\]/g, "")
      .replace(/[*_`#>|]/g, " ")
      .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
      .replace(/\s+/g, " ")
      .trim();
    out[out.length - 1].text += (out[out.length - 1].text ? " " : "") + text;
  }
  return out.filter((s) => s.heading || s.text);
}

/** Split `---` front matter (simple `key: value` lines) from the body. */
export function frontMatter(source: string): { data: Record<string, string>; body: string } {
  const match = /^---\n([\s\S]*?)\n---\n?/.exec(source);
  if (!match) return { data: {}, body: source };
  const data: Record<string, string> = {};
  for (const line of match[1].split("\n")) {
    const m = /^([a-z_]+):\s*(.*)$/.exec(line.trim());
    if (m) data[m[1]] = m[2].replace(/^["']|["']$/g, "");
  }
  return { data, body: source.slice(match[0].length) };
}
