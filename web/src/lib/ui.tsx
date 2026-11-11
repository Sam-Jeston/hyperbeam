// Shared building blocks: routing, time, markdown, and small components.

import { type ReactNode, useEffect, useState, useSyncExternalStore } from "react";
import type { Session, SessionStatus, User } from "./api";

// ------------------------------------------------------------------ routing

const routeListeners = new Set<() => void>();
window.addEventListener("popstate", () => routeListeners.forEach((fn) => fn()));

export function navigate(path: string) {
  if (path !== window.location.pathname + window.location.search) {
    window.history.pushState(null, "", path);
    routeListeners.forEach((fn) => fn());
    window.scrollTo(0, 0);
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

export function A(props: { href: string; className?: string; children: ReactNode; title?: string }) {
  return (
    <a
      href={props.href}
      className={props.className}
      title={props.title}
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

export function clock(iso: string): string {
  return new Date(iso).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

/** Re-render periodically so relative times stay honest. */
export function useTick(ms = 30000) {
  const [, setN] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setN((n) => n + 1), ms);
    return () => clearInterval(id);
  }, [ms]);
}

// --------------------------------------------------------------- components

const AVATAR_HUES = [250, 200, 160, 20, 330, 280, 100, 45];

export function Avatar({ user, size = 22 }: { user?: User | null; size?: number }) {
  const name = user?.name ?? "?";
  const initials = name.split(/\s+/).map((p) => p[0]).slice(0, 2).join("").toUpperCase();
  let hash = 0;
  for (const ch of user?.id ?? name) hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
  const hue = AVATAR_HUES[hash % AVATAR_HUES.length];
  return (
    <span
      className="avatar"
      title={name}
      style={{ width: size, height: size, fontSize: size * 0.42, background: `hsl(${hue} 55% 46%)` }}
    >
      {initials}
    </span>
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

export function StatusDot({ status }: { status: SessionStatus }) {
  return <span className={`dot dot-${status}`} title={STATUS_LABEL[status]} />;
}

export function StatusPill({ session }: { session: Pick<Session, "status"> }) {
  return (
    <span className={`pill pill-${session.status}`}>
      <StatusDot status={session.status} />
      {STATUS_LABEL[session.status]}
    </span>
  );
}

export function Modal(props: { title: string; onClose: () => void; children: ReactNode; wide?: boolean }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && props.onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [props.onClose]);
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && props.onClose()}>
      <div className={`modal ${props.wide ? "modal-wide" : ""}`} role="dialog" aria-label={props.title}>
        <header>
          <h2>{props.title}</h2>
          <button className="icon-btn" onClick={props.onClose} aria-label="Close">×</button>
        </header>
        <div className="modal-body">{props.children}</div>
      </div>
    </div>
  );
}

export function ErrorNote({ error }: { error: string | null | undefined }) {
  return error ? <div className="error-note" role="alert">{error}</div> : null;
}

export function Empty(props: { title: string; children?: ReactNode }) {
  return (
    <div className="empty">
      <strong>{props.title}</strong>
      {props.children && <p>{props.children}</p>}
    </div>
  );
}

/** Run an async action with busy and error state. */
export function useAction() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
      return true;
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
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

export function Markdown({ text }: { text: string }) {
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
  return <div className="md">{blocks}</div>;
}
