import { ArrowLeft, ArrowRight, ChevronRight, Menu, Moon, Pencil, Search, Sun, X } from "lucide-react";
import type { ReactNode } from "react";
import { REPO_URL, SITE_URL } from "./config";
import { href, type Heading } from "./markdown";
import { NAV } from "./nav";

export const cx = (...c: (string | false | null | undefined)[]) => c.filter(Boolean).join(" ");

export interface PageMeta {
  slug: string;
  title: string;
  description: string;
}

export function BrandMark({ size = 24, className }: { size?: number; className?: string }) {
  return (
    <svg viewBox="0 0 40 40" width={size} height={size} className={cx("shrink-0", className)} aria-hidden="true">
      <circle cx="20" cy="20" r="16.5" fill="none" stroke="var(--primary)" strokeWidth="3" />
      <circle cx="14.5" cy="21" r="5.2" fill="var(--primary)" />
      <circle cx="25.5" cy="13.8" r="3.3" fill="var(--second)" />
      <circle cx="25.8" cy="26.4" r="3.3" fill="var(--second)" />
    </svg>
  );
}

function GitHubIcon() {
  return (
    <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" fill="currentColor">
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z" />
    </svg>
  );
}

const pageHref = (slug: string) => href(slug ? `/${slug}` : "/");

function Header() {
  return (
    <header className="sticky top-0 z-40 border-b border-border bg-background/85 backdrop-blur-md">
      <div className="flex h-14 items-center gap-3 px-4 sm:px-6">
        <button type="button" data-sidebar-open className="-ml-1.5 grid size-9 place-items-center rounded-md text-muted hover:bg-surface-2 lg:hidden" aria-label="Open navigation" aria-controls="sidebar" aria-expanded="false">
          <Menu className="size-5" />
        </button>
        <a href={pageHref("")} className="flex items-center gap-2" aria-label="Noemune docs home">
          <BrandMark size={24} />
          <span className="font-display text-[18px] font-bold tracking-[-0.035em]">noemune</span>
          <span className="rounded-md bg-accent px-1.5 py-0.5 text-[11.5px] font-semibold text-accent-foreground">Docs</span>
        </a>
        <button
          type="button"
          data-search-open
          className="ml-auto flex h-9 w-9 items-center justify-center gap-2 rounded-md border border-border-strong bg-surface/70 text-left text-[13.5px] text-faint transition-colors hover:border-faint hover:text-muted md:ml-10 md:w-full md:max-w-[340px] md:justify-start md:px-3"
          aria-label="Search the docs"
        >
          <Search className="size-4 shrink-0" />
          <span className="hidden flex-1 truncate md:block">Search the docs</span>
          <kbd className="hidden rounded border border-border-strong bg-surface-2 px-1.5 font-sans text-[11px] text-muted sm:inline">/</kbd>
        </button>
        <nav className="flex items-center gap-1 md:ml-auto" aria-label="Site">
          <a href={SITE_URL} className="hidden rounded-md px-2.5 py-1.5 text-[13.5px] text-muted hover:text-foreground md:inline">Product</a>
          {REPO_URL && <a href={REPO_URL} className="grid size-9 place-items-center rounded-md text-muted hover:bg-surface-2 hover:text-foreground" aria-label="Source on GitHub"><GitHubIcon /></a>}
          <button type="button" data-theme-toggle className="grid size-9 place-items-center rounded-md text-muted hover:bg-surface-2 hover:text-foreground" aria-label="Switch between light and dark">
            <Sun className="hidden size-4 [.dark_&]:block" />
            <Moon className="hidden size-4 [.light_&]:block" />
          </button>
        </nav>
      </div>
    </header>
  );
}

function Sidebar({ current, titles }: { current: string; titles: Record<string, string> }) {
  return (
    <aside
      id="sidebar"
      data-sidebar
      className="fixed inset-y-0 left-0 z-50 hidden w-[min(300px,86vw)] overflow-y-auto border-r border-border bg-surface px-3 pt-4 pb-10 shadow-pop data-[open]:block lg:sticky lg:top-14 lg:z-0 lg:block lg:h-[calc(100dvh-56px)] lg:w-auto lg:bg-transparent lg:pt-6 lg:shadow-none"
    >
      <div className="mb-3 flex items-center justify-between px-2 lg:hidden">
        <span className="font-display text-[15px] font-semibold">Contents</span>
        <button type="button" data-sidebar-close className="grid size-8 place-items-center rounded-md text-muted hover:bg-surface-2" aria-label="Close navigation"><X className="size-4" /></button>
      </div>
      <nav className="grid gap-6" aria-label="Docs">
        <a href={pageHref("")} aria-current={current === "" ? "page" : undefined} className={cx("rounded-md px-2.5 py-1.5 text-[13.5px] font-medium", current === "" ? "bg-accent text-accent-foreground" : "text-muted hover:text-foreground")}>
          Overview
        </a>
        {NAV.map((group) => (
          <div key={group.title}>
            <div className="mb-1.5 px-2.5 text-[11.5px] font-semibold uppercase tracking-[0.07em] text-faint">{group.title}</div>
            <ul className="grid gap-px border-l border-border pl-0 ml-2.5">
              {group.pages.map((slug) => {
                const active = slug === current;
                return (
                  <li key={slug}>
                    <a
                      href={pageHref(slug)}
                      aria-current={active ? "page" : undefined}
                      className={cx(
                        "-ml-px block border-l px-3 py-1.5 text-[13.5px] transition-colors",
                        active ? "border-primary font-medium text-foreground" : "border-transparent text-muted hover:border-border-strong hover:text-foreground",
                      )}
                    >
                      {titles[slug]}
                    </a>
                  </li>
                );
              })}
            </ul>
          </div>
        ))}
      </nav>
    </aside>
  );
}

function Toc({ headings }: { headings: Heading[] }) {
  if (headings.length < 2) return <div className="hidden xl:block" />;
  return (
    <aside className="hidden xl:block">
      <nav className="sticky top-14 max-h-[calc(100dvh-56px)] overflow-y-auto pt-10 pb-10" aria-label="On this page">
        <div className="mb-2.5 text-[11.5px] font-semibold uppercase tracking-[0.07em] text-faint">On this page</div>
        <ul className="grid gap-1 text-[13px]" data-toc>
          {headings.map((h) => (
            <li key={h.id} className={h.depth === 3 ? "pl-3" : undefined}>
              <a href={`#${h.id}`} data-toc-link={h.id} className="block py-0.5 text-muted transition-colors hover:text-foreground data-[active]:text-primary">
                {h.text}
              </a>
            </li>
          ))}
        </ul>
      </nav>
    </aside>
  );
}

function Pager({ prev, next }: { prev?: PageMeta; next?: PageMeta }) {
  if (!prev && !next) return null;
  const card = (p: PageMeta, dir: "prev" | "next") => (
    <a
      href={pageHref(p.slug)}
      className={cx("group grid gap-0.5 rounded-lg border border-border bg-surface px-4 py-3 transition-colors hover:border-primary/60", dir === "next" && "text-right sm:col-start-2")}
    >
      <span className={cx("flex items-center gap-1 text-[12px] text-faint", dir === "next" && "justify-end")}>
        {dir === "prev" ? <><ArrowLeft className="size-3.5" /> Previous</> : <>Next <ArrowRight className="size-3.5" /></>}
      </span>
      <span className="font-medium text-foreground group-hover:text-primary">{p.title}</span>
    </a>
  );
  return (
    <nav className="mt-14 grid gap-3 sm:grid-cols-2" aria-label="Previous and next pages">
      {prev && card(prev, "prev")}
      {next && card(next, "next")}
    </nav>
  );
}

function SearchDialog() {
  return (
    <dialog data-search className="m-auto mt-[10vh] w-[min(620px,calc(100vw-32px))] overflow-hidden rounded-xl border border-border bg-elevated p-0 text-foreground shadow-pop backdrop:bg-overlay backdrop:backdrop-blur-[2px]">
      <div className="flex items-center gap-2.5 border-b border-border px-4">
        <Search className="size-4 shrink-0 text-faint" />
        <input
          data-search-input
          type="search"
          placeholder="Search the docs"
          aria-label="Search the docs"
          autoComplete="off"
          spellCheck={false}
          className="h-13 w-full bg-transparent text-[15px] outline-none placeholder:text-faint [&::-webkit-search-cancel-button]:hidden"
        />
        <kbd className="rounded border border-border-strong bg-surface-2 px-1.5 text-[11px] text-muted">Esc</kbd>
      </div>
      <ul data-search-results role="listbox" aria-label="Results" className="max-h-[60vh] overflow-y-auto p-2" />
      <p data-search-empty className="px-4 pt-2 pb-5 text-[13.5px] text-muted">Type to search pages and sections.</p>
    </dialog>
  );
}

export function DocPage(props: {
  meta: PageMeta;
  group?: string;
  html?: string;
  children?: ReactNode;
  headings: Heading[];
  titles: Record<string, string>;
  prev?: PageMeta;
  next?: PageMeta;
  editPath?: string;
  wide?: boolean;
}) {
  return (
    <>
      <a href="#content" className="sr-only focus:not-sr-only focus:fixed focus:left-3 focus:top-3 focus:z-50 focus:rounded-md focus:bg-surface focus:px-3 focus:py-2">Skip to content</a>
      <Header />
      <div data-sidebar-backdrop hidden className="fixed inset-0 z-40 bg-overlay lg:hidden" />
      <div className="mx-auto grid max-w-[1440px] lg:grid-cols-[260px_minmax(0,1fr)] xl:grid-cols-[260px_minmax(0,1fr)_220px] lg:gap-10 lg:px-6 xl:gap-12">
        <Sidebar current={props.meta.slug} titles={props.titles} />
        <main id="content" className={cx("min-w-0 px-4 pt-8 pb-20 sm:px-8 lg:px-0 lg:pt-10", props.wide ? "" : "max-w-[760px]")}>
          {props.group && (
            <div className="mb-3 flex items-center gap-1 text-[13px] text-muted">
              {props.group} <ChevronRight className="size-3.5 text-faint" />
            </div>
          )}
          <h1 className="font-display text-[34px] font-semibold leading-tight tracking-[-0.03em] text-balance">{props.meta.title}</h1>
          {props.meta.description && <p className="mt-2 text-[17px] leading-relaxed text-muted text-pretty">{props.meta.description}</p>}
          <div className="mt-8">
            {props.html ? <div className="prose" dangerouslySetInnerHTML={{ __html: props.html }} /> : props.children}
          </div>
          {props.editPath && REPO_URL && (
            <div className="mt-14 border-t border-border pt-5 text-[13px]">
              <a href={`${REPO_URL}/edit/${process.env.NOEMUNE_REPO_BRANCH ?? "main"}/docs/content/${props.editPath}`} className="inline-flex items-center gap-1.5 text-muted hover:text-foreground">
                <Pencil className="size-3.5" /> Edit this page
              </a>
            </div>
          )}
          <Pager prev={props.prev} next={props.next} />
        </main>
        <Toc headings={props.headings} />
      </div>
      <SearchDialog />
    </>
  );
}
