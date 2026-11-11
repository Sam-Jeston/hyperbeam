// The docs' interactive pieces: theme, mobile navigation, copy buttons,
// the "On this page" highlight, and search. Pages read fine without it.

declare const DOCS_BASE: string;

const root = document.documentElement;
const base = DOCS_BASE === "/" ? "" : DOCS_BASE;

// ---------------------------------------------------------------- theme

document.querySelectorAll("[data-theme-toggle]").forEach((button) =>
  button.addEventListener("click", () => {
    const next = root.classList.contains("light") ? "dark" : "light";
    root.className = next;
    try {
      localStorage.setItem("hyperbeam:theme", next);
    } catch {
      // Private windows may refuse; the choice still applies on this page.
    }
  }),
);

// ------------------------------------------------------- mobile sidebar

const sidebar = document.querySelector<HTMLElement>("[data-sidebar]");
const backdrop = document.querySelector<HTMLElement>("[data-sidebar-backdrop]");
const sidebarOpener = document.querySelector<HTMLButtonElement>("[data-sidebar-open]");
const setSidebar = (open: boolean) => {
  sidebar?.toggleAttribute("data-open", open);
  if (backdrop) backdrop.hidden = !open;
  sidebarOpener?.setAttribute("aria-expanded", String(open));
  if (open) sidebar?.querySelector<HTMLElement>("[aria-current=page], a")?.focus();
};
sidebarOpener?.addEventListener("click", () => setSidebar(true));
document.querySelector("[data-sidebar-close]")?.addEventListener("click", () => setSidebar(false));
backdrop?.addEventListener("click", () => setSidebar(false));
document.addEventListener("keydown", (e) => e.key === "Escape" && sidebar?.hasAttribute("data-open") && setSidebar(false));
// Bring the current page into view in the sidebar, without scrolling the page itself.
const current = sidebar?.querySelector<HTMLElement>("[aria-current=page]");
if (sidebar && current) sidebar.scrollTop = Math.max(0, current.offsetTop - sidebar.clientHeight / 2);

// --------------------------------------------------------- copy buttons

document.querySelectorAll<HTMLButtonElement>("[data-copy]").forEach((button) =>
  button.addEventListener("click", async () => {
    const label = button.querySelector("span");
    try {
      await navigator.clipboard.writeText(button.dataset.copy ?? "");
      if (label) label.textContent = "Copied";
      button.setAttribute("aria-label", "Copied");
    } catch {
      if (label) label.textContent = "Select to copy";
    }
    setTimeout(() => {
      if (label) label.textContent = "Copy";
      button.setAttribute("aria-label", "Copy code");
    }, 1600);
  }),
);

// ------------------------------------------------- "On this page" spy

const tocLinks = [...document.querySelectorAll<HTMLAnchorElement>("[data-toc-link]")];
if (tocLinks.length) {
  const targets = tocLinks.map((a) => document.getElementById(a.dataset.tocLink!)).filter(Boolean) as HTMLElement[];
  const update = () => {
    const line = 120;
    let current = targets[0];
    for (const t of targets) if (t.getBoundingClientRect().top <= line) current = t;
    // At the very bottom, the last section is the one being read.
    if (innerHeight + scrollY >= document.body.scrollHeight - 4) current = targets[targets.length - 1];
    tocLinks.forEach((a) => a.toggleAttribute("data-active", a.dataset.tocLink === current?.id));
  };
  addEventListener("scroll", update, { passive: true });
  update();
}

// ---------------------------------------------------------------- search

interface Doc {
  slug: string;
  title: string;
  description: string;
  group: string;
  sections: { heading: string; id: string; text: string }[];
}

const dialog = document.querySelector<HTMLDialogElement>("[data-search]")!;
const input = document.querySelector<HTMLInputElement>("[data-search-input]")!;
const list = document.querySelector<HTMLUListElement>("[data-search-results]")!;
const empty = document.querySelector<HTMLElement>("[data-search-empty]")!;
let index: Promise<Doc[]> | null = null;
let selected = 0;
let results: { url: string }[] = [];

const load = () => (index ??= fetch(`${base}/search-index.json`).then((r) => r.json()));
const esc = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);

function mark(text: string, terms: string[]) {
  let out = esc(text);
  for (const t of terms) out = out.replace(new RegExp(`(${t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")})`, "gi"), "<mark>$1</mark>");
  return out;
}

function snippet(text: string, terms: string[]) {
  const lower = text.toLowerCase();
  const at = Math.max(0, Math.min(...terms.map((t) => lower.indexOf(t)).filter((i) => i >= 0), text.length) - 50);
  const cut = text.slice(at, at + 150);
  return (at > 0 ? "…" : "") + cut + (at + 150 < text.length ? "…" : "");
}

async function search(query: string) {
  const terms = query.toLowerCase().split(/\s+/).filter((t) => t.length > 1 || /\d/.test(t));
  if (!terms.length) {
    list.innerHTML = "";
    empty.hidden = false;
    empty.textContent = "Type to search pages and sections.";
    results = [];
    return;
  }
  const docs = await load();
  const scored: { score: number; url: string; title: string; context: string; text: string }[] = [];
  for (const doc of docs) {
    for (const section of doc.sections) {
      const title = doc.title.toLowerCase();
      const heading = section.heading.toLowerCase();
      const body = section.text.toLowerCase();
      let score = 0;
      for (const t of terms) {
        const hit = (title.includes(t) ? 6 : 0) + (heading.includes(t) ? 4 : 0) + (body.includes(t) ? 1 : 0);
        if (!hit) {
          score = 0;
          break;
        }
        score += hit;
      }
      if (!section.heading) score += 1; // Prefer the page itself when it matches as well as a section.
      if (score) {
        scored.push({
          score,
          url: `${base}/${doc.slug}${section.id ? `#${section.id}` : ""}`.replace(/\/+/g, "/").replace(/\/$/, "") || "/",
          title: section.heading || doc.title,
          context: section.heading ? doc.title : doc.group,
          text: snippet(section.text || doc.description, terms),
        });
      }
    }
  }
  scored.sort((a, b) => b.score - a.score);
  const top = scored.slice(0, 10);
  results = top;
  selected = 0;
  empty.hidden = top.length > 0;
  empty.textContent = `No results for “${query}”. Try a shorter or different word.`;
  list.innerHTML = top
    .map(
      (r, i) => `<li><a href="${r.url}" class="search-result block rounded-lg px-3 py-2.5" role="option" id="sr-${i}" aria-selected="${i === 0}">
        <div class="text-[11.5px] text-faint">${esc(r.context)}</div>
        <div class="text-[14px] font-medium">${mark(r.title, terms)}</div>
        <div class="mt-0.5 line-clamp-2 text-[12.5px] leading-snug text-muted">${mark(r.text, terms)}</div>
      </a></li>`,
    )
    .join("");
}

function select(i: number) {
  const items = [...list.querySelectorAll<HTMLElement>("[role=option]")];
  if (!items.length) return;
  selected = (i + items.length) % items.length;
  items.forEach((el, n) => el.setAttribute("aria-selected", String(n === selected)));
  items[selected].scrollIntoView({ block: "nearest" });
  input.setAttribute("aria-activedescendant", `sr-${selected}`);
}

function openSearch() {
  if (dialog.open) return;
  dialog.showModal();
  input.select();
  void load();
}

document.querySelectorAll("[data-search-open]").forEach((b) => b.addEventListener("click", openSearch));
document.addEventListener("keydown", (e) => {
  const typing = (e.target as HTMLElement).closest("input, textarea, [contenteditable]");
  if ((e.key === "k" && (e.metaKey || e.ctrlKey)) || (e.key === "/" && !typing)) {
    e.preventDefault();
    openSearch();
  }
});
input.addEventListener("input", () => void search(input.value));
input.addEventListener("keydown", (e) => {
  if (e.key === "ArrowDown") (e.preventDefault(), select(selected + 1));
  else if (e.key === "ArrowUp") (e.preventDefault(), select(selected - 1));
  else if (e.key === "Enter" && results[selected]) {
    e.preventDefault();
    location.href = results[selected].url;
    dialog.close();
  }
});
dialog.addEventListener("click", (e) => {
  if (e.target === dialog) dialog.close();
  if ((e.target as HTMLElement).closest("a")) dialog.close();
});
