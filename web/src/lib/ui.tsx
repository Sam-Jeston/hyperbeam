// Shared building blocks: routing, time, data hooks, markdown, and the
// app-specific components built on the kit in components/ui.

import { Check, ChevronRight, Copy } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { toast } from "sonner";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Tooltip } from "../components/ui/menu";
import { Badge, Notice } from "../components/ui/surface";
import type { Session, SessionStatus, User } from "./api";
import { cn } from "./utils";

// ------------------------------------------------------------------ routing

const routeListeners = new Set<() => void>();
window.addEventListener("popstate", () => routeListeners.forEach((fn) => fn()));

export function navigate(path: string) {
  if (path !== window.location.pathname + window.location.search) {
    window.history.pushState(null, "", path);
    routeListeners.forEach((fn) => fn());
    document.getElementById("main-scroll")?.scrollTo(0, 0);
  }
}

export function usePath(): string {
  return useSyncExternalStore(
    (fn) => {
      routeListeners.add(fn);
      return () => routeListeners.delete(fn);
    },
    () => window.location.pathname,
  );
}

export function A(props: { href: string; className?: string; children: ReactNode; title?: string; "aria-current"?: "page" }) {
  return (
    <a
      href={props.href}
      className={props.className}
      title={props.title}
      aria-current={props["aria-current"]}
      onClick={(e) => {
        if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
        e.preventDefault();
        navigate(props.href);
      }}
    >
      {props.children}
    </a>
  );
}

/** An external link that opens in a new tab. */
export function Ext({ href, className, children }: { href: string; className?: string; children: ReactNode }) {
  return (
    <a href={href} target="_blank" rel="noreferrer noopener" className={cn("text-primary underline-offset-2 hover:underline", className)}>
      {children}
    </a>
  );
}

// --------------------------------------------------------------------- time

export function timeAgo(iso: string | null | undefined): string {
  if (!iso) return "";
  const seconds = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000);
  if (seconds < 45) return "just now";
  if (seconds < 3600) return `${Math.round(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.round(seconds / 3600)}h ago`;
  if (seconds < 86400 * 14) return `${Math.round(seconds / 86400)}d ago`;
  return new Date(iso).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

export function fullTime(iso: string): string {
  return new Date(iso).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

export function clock(iso: string): string {
  return new Date(iso).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

/** "Today", "Yesterday", or a date, for grouping a feed by day. */
export function dayLabel(iso: string): string {
  const d = new Date(iso);
  const today = new Date();
  const start = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((start(today) - start(d)) / 86400_000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  return d.toLocaleDateString(undefined, { weekday: "long", month: "short", day: "numeric", year: d.getFullYear() === today.getFullYear() ? undefined : "numeric" });
}

/** A relative time that shows the exact time on hover. */
export function Time({ iso, className }: { iso: string | null | undefined; className?: string }) {
  if (!iso) return null;
  return (
    <Tooltip content={fullTime(iso)}>
      <time dateTime={iso} className={cn("whitespace-nowrap tabular-nums", className)}>{timeAgo(iso)}</time>
    </Tooltip>
  );
}

/** Re-render periodically so relative times stay honest. */
export function useTick(ms = 30000) {
  const [, setN] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setN((n) => n + 1), ms);
    return () => clearInterval(id);
  }, [ms]);
}

export function useDocumentTitle(title: string | null | undefined) {
  useEffect(() => {
    document.title = title ? `${title} · Hyperbeam` : "Hyperbeam";
  }, [title]);
}

// ----------------------------------------------------------- global actions

let composerOpen = false;
const composerListeners = new Set<() => void>();
/** Open "New work" from anywhere: the sidebar, a page, or the C shortcut. */
export function openComposer(open = true) {
  composerOpen = open;
  composerListeners.forEach((fn) => fn());
}
export function useComposerOpen() {
  return useSyncExternalStore((fn) => (composerListeners.add(fn), () => composerListeners.delete(fn)), () => composerOpen);
}

/** True when the keyboard is in a text field, so single-key shortcuts stay out of the way. */
export function typing(e: KeyboardEvent) {
  const el = e.target as HTMLElement | null;
  return !!el && (el.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName) || !!el.closest("[role=dialog],[role=menu],[role=listbox]"));
}

export const isMac = /Mac|iPhone|iPad/.test(navigator.platform);
export const modKey = isMac ? "⌘" : "Ctrl";

/** ⌘/Ctrl+Enter submits. */
export const isSubmitKey = (e: React.KeyboardEvent) => e.key === "Enter" && (e.metaKey || e.ctrlKey) && !e.nativeEvent.isComposing;

// --------------------------------------------------------------- components

const SYSTEM_NAMES: Record<string, string> = { github: "GitHub", linear: "Linear", notion: "Notion", posthog: "PostHog", sentry: "Sentry" };
/** How to write an integration's name: "GitHub", not "Github". */
export const systemName = (system: string) => SYSTEM_NAMES[system] ?? system.charAt(0).toUpperCase() + system.slice(1);

const AVATAR_HUES = [282, 340, 160, 200, 30, 250, 120, 60];

export function Avatar({ user, size = 22, className }: { user?: Pick<User, "id" | "name"> | null; size?: number; className?: string }) {
  const name = user?.name ?? "?";
  const initials = name.split(/\s+/).filter(Boolean).map((p) => p[0]).slice(0, 2).join("").toUpperCase() || "?";
  let hash = 0;
  for (const ch of user?.id ?? name) hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
  const hue = AVATAR_HUES[hash % AVATAR_HUES.length];
  return (
    <Tooltip content={name}>
      <span
        className={cn("inline-grid shrink-0 select-none place-items-center rounded-full font-semibold text-white ring-2 ring-surface", className)}
        style={{ width: size, height: size, fontSize: Math.max(9, size * 0.4), background: `oklch(0.58 0.12 ${hue})` }}
        aria-label={name}
      >
        {initials}
      </span>
    </Tooltip>
  );
}

export function AvatarStack({ users, size = 20, max = 3 }: { users: (User | undefined)[]; size?: number; max?: number }) {
  const shown = users.filter(Boolean).slice(0, max) as User[];
  const extra = users.filter(Boolean).length - shown.length;
  return (
    <div className="flex items-center -space-x-1.5">
      {shown.map((u) => <Avatar key={u.id} user={u} size={size} />)}
      {extra > 0 && (
        <span className="inline-grid place-items-center rounded-full bg-surface-2 text-[10px] font-semibold text-muted ring-2 ring-surface" style={{ width: size, height: size }}>
          +{extra}
        </span>
      )}
    </div>
  );
}

export const STATUS_LABEL: Record<SessionStatus, string> = {
  pending: "Queued",
  starting: "Starting",
  running: "Working",
  idle: "Waiting for input",
  completed: "Asleep",
  failed: "Failed",
  cancelled: "Stopped",
};

const STATUS_HELP: Record<SessionStatus, string> = {
  pending: "Waiting for a free sandbox.",
  starting: "Preparing its sandbox.",
  running: "The agent is working.",
  idle: "The agent finished its turn and is waiting for a reply.",
  completed: "Its sandbox has shut down. Send a message to wake it in a fresh one.",
  failed: "The session stopped because of an error.",
  cancelled: "Someone stopped this agent.",
};

const STATUS_TONE: Record<SessionStatus, "run" | "wait" | "idle" | "fail" | "neutral"> = {
  pending: "wait",
  starting: "wait",
  running: "run",
  idle: "idle",
  completed: "neutral",
  failed: "fail",
  cancelled: "neutral",
};

const DOT_BG = { run: "bg-run", wait: "bg-wait", idle: "bg-idle", fail: "bg-fail", neutral: "bg-faint" };

export function StatusDot({ status, className }: { status: SessionStatus; className?: string }) {
  const tone = STATUS_TONE[status];
  return (
    <span
      role="img"
      aria-label={STATUS_LABEL[status]}
      title={STATUS_LABEL[status]}
      className={cn("inline-block size-2 shrink-0 rounded-full", DOT_BG[tone], status === "running" && "animate-pulse-ring", className)}
    />
  );
}

export function StatusBadge({ session }: { session: Pick<Session, "status"> }) {
  const tone = STATUS_TONE[session.status];
  return (
    <Tooltip content={STATUS_HELP[session.status]}>
      <Badge tone={tone}>
        <span className={cn("size-1.5 rounded-full", DOT_BG[tone], session.status === "running" && "animate-pulse-ring")} />
        {STATUS_LABEL[session.status]}
      </Badge>
    </Tooltip>
  );
}

export function ErrorNote({ error, title }: { error: string | null | undefined; title?: string }) {
  return error ? <Notice tone="error" title={title}>{error}</Notice> : null;
}

/** A read-only value with a copy button, for links and keys shown once. */
export function CopyField({ value, label = "Copy" }: { value: string; label?: string }) {
  const [copied, setCopied] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    } catch {
      input.current?.select();
      toast("Press " + modKey + "+C to copy");
    }
  };
  return (
    <div className="flex gap-2">
      <Input ref={input} readOnly mono value={value} onFocus={(e) => e.target.select()} aria-label="Value to copy" />
      <Button variant={copied ? "secondary" : "primary"} onClick={() => void copy()} className="w-24">
        {copied ? <Check /> : <Copy />}
        {copied ? "Copied" : label}
      </Button>
    </div>
  );
}

// ------------------------------------------------------------- page layout

export function Breadcrumbs({ items }: { items: { label: ReactNode; href: string }[] }) {
  return (
    <nav aria-label="Breadcrumb" className="mb-2 flex min-w-0 flex-wrap items-center gap-1 text-[12.5px] text-muted">
      {items.map((item, i) => (
        <span key={item.href} className="flex min-w-0 items-center gap-1">
          {i > 0 && <ChevronRight className="size-3.5 shrink-0 text-faint" />}
          <A href={item.href} className="truncate rounded-sm hover:text-foreground">{item.label}</A>
        </span>
      ))}
    </nav>
  );
}

/** The top of every page: title, a line of context, and the page's actions. */
export function PageHeader(props: { title: ReactNode; description?: ReactNode; actions?: ReactNode; breadcrumbs?: { label: ReactNode; href: string }[]; meta?: ReactNode }) {
  return (
    <header className="mb-6">
      {props.breadcrumbs && <Breadcrumbs items={props.breadcrumbs} />}
      <div className="flex flex-wrap items-start justify-between gap-x-6 gap-y-3">
        <div className="min-w-0 flex-1">
          <h1 className="font-display text-[24px] font-semibold leading-tight tracking-[-0.02em] [overflow-wrap:anywhere]">{props.title}</h1>
          {props.description && <p className="mt-1 max-w-[70ch] text-[13.5px] text-muted">{props.description}</p>}
          {props.meta && <div className="mt-2.5 flex flex-wrap items-center gap-2 text-[13px] text-muted">{props.meta}</div>}
        </div>
        {props.actions && <div className="flex flex-wrap items-center gap-2">{props.actions}</div>}
      </div>
    </header>
  );
}

export function Page({ children, width = "default", className }: { children: ReactNode; width?: "default" | "narrow" | "wide"; className?: string }) {
  return (
    <div
      className={cn(
        "mx-auto w-full px-4 pt-6 pb-16 sm:px-8 sm:pt-8",
        width === "narrow" ? "max-w-[760px]" : width === "wide" ? "max-w-[1480px]" : "max-w-[1180px]",
        className,
      )}
    >
      {children}
    </div>
  );
}

/** Section heading inside a page. */
export function SectionTitle({ children, action, description }: { children: ReactNode; action?: ReactNode; description?: ReactNode }) {
  return (
    <div className="mb-3 flex items-end justify-between gap-4">
      <div>
        <h2 className="font-display text-[15.5px] font-semibold tracking-[-0.01em]">{children}</h2>
        {description && <p className="mt-0.5 text-[13px] text-muted">{description}</p>}
      </div>
      {action}
    </div>
  );
}

// -------------------------------------------------------------- data hooks

/** Run an async action with busy and error state. Pass `success` to confirm with a toast. */
export function useAction() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async (fn: () => Promise<unknown>, opts?: { success?: string; toastError?: boolean }) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
      if (opts?.success) toast.success(opts.success);
      return true;
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      if (opts?.toastError) toast.error(message);
      else setError(message);
      return false;
    } finally {
      setBusy(false);
    }
  };
  return { busy, error, run, setError };
}

/** Fetch on mount and whenever `deps` change. */
export function useLoad<T>(load: () => Promise<T>, deps: unknown[]): { data: T | null; error: string | null; reload: () => void } {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [n, setN] = useState(0);
  useEffect(() => {
    let cancelled = false;
    load().then(
      (d) => !cancelled && (setData(d), setError(null)),
      (e) => !cancelled && setError(e instanceof Error ? e.message : String(e)),
    );
    return () => {
      cancelled = true;
    };
  }, [...deps, n]);
  return { data, error, reload: () => setN((x) => x + 1) };
}

// ----------------------------------------------------------------- markdown
// Agent output is untrusted. This renders a useful subset of markdown
// straight to React elements; nothing is ever injected as HTML.

function inline(text: string, key: string): ReactNode[] {
  const out: ReactNode[] = [];
  const pattern = /(`[^`\n]+`)|(\*\*[^*\n]+\*\*)|(\[[^\]\n]+\]\((https?:\/\/[^\s)]+)\))|(https?:\/\/[^\s<>)\]]+)/g;
  let last = 0;
  let match: RegExpExecArray | null;
  let i = 0;
  while ((match = pattern.exec(text))) {
    if (match.index > last) out.push(text.slice(last, match.index));
    const k = `${key}-${i++}`;
    if (match[1]) out.push(<code key={k}>{match[1].slice(1, -1)}</code>);
    else if (match[2]) out.push(<strong key={k}>{match[2].slice(2, -2)}</strong>);
    else if (match[3]) {
      const label = match[3].slice(1, match[3].indexOf("]("));
      out.push(<a key={k} href={match[4]} target="_blank" rel="noreferrer noopener">{label}</a>);
    } else if (match[5]) {
      out.push(<a key={k} href={match[5]} target="_blank" rel="noreferrer noopener">{match[5]}</a>);
    }
    last = match.index + match[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

export function Markdown({ text, className }: { text: string; className?: string }) {
  const blocks: ReactNode[] = [];
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  let i = 0;
  let key = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (line.trimStart().startsWith("```")) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !lines[i].trimStart().startsWith("```")) body.push(lines[i++]);
      i++;
      blocks.push(<pre key={key++}><code>{body.join("\n")}</code></pre>);
      continue;
    }
    const heading = /^(#{1,4})\s+(.*)$/.exec(line);
    if (heading) {
      const level = Math.min(heading[1].length + 2, 6);
      const Tag = `h${level}` as "h3";
      blocks.push(<Tag key={key++}>{inline(heading[2], `h${key}`)}</Tag>);
      i++;
      continue;
    }
    if (/^\s*([-*]|\d+\.)\s+/.test(line)) {
      const ordered = /^\s*\d+\./.test(line);
      const items: ReactNode[] = [];
      while (i < lines.length && /^\s*([-*]|\d+\.)\s+/.test(lines[i])) {
        items.push(<li key={items.length}>{inline(lines[i].replace(/^\s*([-*]|\d+\.)\s+/, ""), `li${key}-${items.length}`)}</li>);
        i++;
      }
      blocks.push(ordered ? <ol key={key++}>{items}</ol> : <ul key={key++}>{items}</ul>);
      continue;
    }
    if (line.trim() === "") {
      i++;
      continue;
    }
    const para: string[] = [];
    while (i < lines.length && lines[i].trim() !== "" && !lines[i].trimStart().startsWith("```") && !/^(#{1,4})\s+/.test(lines[i]) && !/^\s*([-*]|\d+\.)\s+/.test(lines[i])) {
      para.push(lines[i++]);
    }
    blocks.push(<p key={key++}>{inline(para.join("\n"), `p${key}`)}</p>);
  }
  return <div className={cn("prose-agent", className)}>{blocks}</div>;
}
