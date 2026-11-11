import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { api, isLive, type Session, type SessionEvent } from "../lib/api";
import { useStore } from "../lib/store";
import { A, Avatar, clock, ErrorNote, Markdown, STATUS_LABEL, StatusPill, timeAgo, useAction, useLoad, useTick } from "../lib/ui";
import { AgentRow } from "./Board";

interface Detail {
  session: Session;
  harness: { name: string; title: string; model: string | null } | null;
  children: Session[];
  parent: { id: string; title: string } | null;
  workstream: { id: string; title: string } | null;
  can_drive: boolean;
}

type Item =
  | { type: "user"; key: number; text: string; author: string; userId: string | null; ts: string }
  | { type: "agent"; key: number; text: string }
  | { type: "thought"; key: number; text: string }
  | { type: "tool"; key: number; data: any }
  | { type: "gate"; key: number; data: any }
  | { type: "note"; key: number; text: string; rule?: boolean }
  | { type: "error"; key: number; text: string };

/** Fold the raw event log into what a person wants to read. */
function fold(events: SessionEvent[]): { items: Item[]; plan: any[] | null } {
  const items: Item[] = [];
  let plan: any[] | null = null;
  // A relaunched harness may reuse tool call ids, so always attach updates to
  // the most recent call with that id.
  const toolIndex = new Map<string, number>();
  for (const event of events) {
    const { kind, data, seq } = event;
    const last = items[items.length - 1];
    switch (kind) {
      case "user_message":
        items.push({ type: "user", key: seq, text: data.text ?? "", author: data.author ?? "Someone", userId: data.user_id ?? null, ts: event.ts });
        break;
      case "agent_message":
        if (typeof data.text !== "string") break;
        if (last?.type === "agent") last.text += data.text;
        else items.push({ type: "agent", key: seq, text: data.text });
        break;
      case "agent_thought":
        if (typeof data.text !== "string") break;
        if (last?.type === "thought") last.text += data.text;
        else items.push({ type: "thought", key: seq, text: data.text });
        break;
      case "tool_call":
        toolIndex.set(data.toolCallId, items.length);
        items.push({ type: "tool", key: seq, data: { ...data } });
        break;
      case "tool_call_update": {
        const index = toolIndex.get(data.toolCallId);
        const target = index === undefined ? undefined : items[index];
        if (target?.type === "tool") {
          for (const [k, v] of Object.entries(data)) if (v !== null && v !== undefined) target.data[k] = v;
        }
        break;
      }
      case "plan":
        plan = Array.isArray(data.entries) ? data.entries : plan;
        break;
      case "gateway":
        items.push({ type: "gate", key: seq, data });
        break;
      case "system":
        items.push({ type: "note", key: seq, text: data.text ?? "" });
        break;
      case "status":
        if (data.status === "cancelled") items.push({ type: "note", key: seq, text: "Stopped", rule: true });
        else if (data.status === "completed") items.push({ type: "note", key: seq, text: "Asleep", rule: true });
        else if (data.status === "failed") items.push({ type: "note", key: seq, text: "Failed", rule: true });
        break;
      case "turn_end":
        if (data.stop_reason === "cancelled") items.push({ type: "note", key: seq, text: "Interrupted", rule: true });
        else if (data.stop_reason === "refusal") items.push({ type: "note", key: seq, text: "The agent declined to continue", rule: true });
        else if (data.stop_reason === "max_tokens" || data.stop_reason === "max_turn_requests") items.push({ type: "note", key: seq, text: "The agent hit its limit for one turn; send a message to continue", rule: true });
        break;
      case "error":
        items.push({ type: "error", key: seq, text: data.text ?? "Unknown error" });
        break;
    }
  }
  return { items, plan };
}

function useEvents(id: string) {
  const [events, setEvents] = useState<SessionEvent[]>([]);
  const [caughtUp, setCaughtUp] = useState(false);
  useEffect(() => {
    let last = 0;
    let es: EventSource | null = null;
    let closed = false;
    const open = () => {
      // Reconnect from where we are, never from the beginning.
      es = new EventSource(`/api/v1/sessions/${id}/stream?after=${last}`);
      const buffer: SessionEvent[] = [];
      let flushTimer: ReturnType<typeof setTimeout> | null = null;
      const flush = () => {
        flushTimer = null;
        const batch = buffer.splice(0);
        if (batch.length) setEvents((prev) => [...prev, ...batch]);
      };
      es.addEventListener("event", (e) => {
        const event: SessionEvent = JSON.parse((e as MessageEvent).data);
        if (event.seq <= last) return;
        last = event.seq;
        buffer.push(event);
        flushTimer ??= setTimeout(flush, 60);
      });
      es.addEventListener("caught_up", () => (flush(), setCaughtUp(true)));
      const reopen = () => {
        es?.close();
        if (!closed) setTimeout(open, 1000);
      };
      es.addEventListener("resync", reopen);
      es.onerror = reopen;
    };
    open();
    return () => {
      closed = true;
      es?.close();
    };
  }, [id]);
  return { events, caughtUp };
}

export function SessionPage({ id }: { id: string }) {
  useTick();
  const users = useStore((s) => s.users);
  const liveSession = useStore((s) => s.sessions[id]);
  const allSessions = useStore((s) => s.sessions);
  const { data, error } = useLoad<Detail>(() => api.get(`/sessions/${id}`), [id]);
  const { events, caughtUp } = useEvents(id);
  const { items, plan } = useMemo(() => fold(events), [events]);
  const [text, setText] = useState("");
  const action = useAction();
  const bottom = useRef<HTMLDivElement>(null);
  const stuck = useRef(true);

  // Follow the transcript while the reader is at the bottom of it.
  useEffect(() => {
    const scroller = document.querySelector(".main");
    if (!scroller) return;
    const onScroll = () => {
      stuck.current = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 140;
    };
    scroller.addEventListener("scroll", onScroll, { passive: true });
    return () => scroller.removeEventListener("scroll", onScroll);
  }, []);
  useEffect(() => {
    if (stuck.current) bottom.current?.scrollIntoView({ block: "end" });
  }, [events.length, caughtUp]);

  if (error) return <div className="page"><ErrorNote error={error} /></div>;
  if (!data) return null;
  const session = liveSession ?? data.session;
  const children = Object.values(allSessions).filter((s) => s.parent_id === id).sort((a, b) => a.created_at.localeCompare(b.created_at));
  const owner = users[session.created_by];
  const live = isLive(session);

  const send = () => {
    const message = text.trim();
    if (!message) return;
    void action.run(async () => {
      await api.post(`/sessions/${id}/messages`, { text: message });
      setText("");
      stuck.current = true;
    });
  };

  return (
    <div className="page">
      <div className="small muted" style={{ marginBottom: 6 }}>
        <A href="/">Board</A> /{" "}
        {data.workstream && <><A href={`/w/${data.workstream.id}`}>{data.workstream.title}</A> / </>}
        {data.parent && <><A href={`/s/${data.parent.id}`}>{data.parent.title}</A> / </>}
      </div>
      <div className="page-head">
        <div className="grow">
          <h1>{session.title}</h1>
          <p className="row-wrap" style={{ marginTop: 6 }}>
            <StatusPill session={session} />
            {session.role === "conductor" && <span className="chip chip-accent">Conductor</span>}
            {session.status === "running" && session.headline && <span className="truncate" style={{ maxWidth: 520 }}>{session.headline}</span>}
          </p>
        </div>
        {data.can_drive && live && (
          <div className="row">
            {session.status === "running" && (
              <button className="btn" onClick={() => void action.run(() => api.post(`/sessions/${id}/cancel`))}>Interrupt</button>
            )}
            <button className="btn btn-danger" onClick={() => void action.run(() => api.post(`/sessions/${id}/stop`))}>Stop</button>
          </div>
        )}
      </div>

      <div className="session-layout">
        <div>
          <div className="transcript" aria-live="polite">
            {items.map((item) => <TranscriptItem key={item.key} item={item} />)}
            {session.status === "running" && <div className="sysline"><span className="dot dot-running" /> Working…</div>}
            {(session.status === "starting" || session.status === "pending") && (
              <div className="sysline"><span className="dot dot-starting" /> {session.status === "pending" ? "Waiting for a free sandbox…" : "Preparing the sandbox…"}</div>
            )}
            <div ref={bottom} />
          </div>

          <div className="composer">
            {data.can_drive ? (
              <div className="composer-box">
                <textarea
                  rows={2}
                  value={text}
                  placeholder={
                    live
                      ? session.status === "running" ? "Queue a follow-up for when this turn ends" : "Reply to the agent"
                      : "Send a message to wake this agent in a fresh sandbox"
                  }
                  onChange={(e) => setText(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                      e.preventDefault();
                      send();
                    }
                  }}
                />
                <div className="spread">
                  <span className="small faint">Enter to send · Shift+Enter for a new line</span>
                  <button className="btn btn-primary btn-sm" disabled={!text.trim() || action.busy} onClick={send}>Send</button>
                </div>
              </div>
            ) : (
              <div className="card card-pad small muted">
                You are watching. This agent holds permissions you do not, so only {owner?.name ?? "its owner"} or an admin can direct it.
                {data.workstream && <> You can comment on <A href={`/w/${data.workstream.id}`}>its workstream</A>.</>}
              </div>
            )}
            <ErrorNote error={action.error} />
          </div>
        </div>

        <aside className="side">
          <div className="card card-pad">
            <h3>About</h3>
            <dl className="kv">
              <dt>Status</dt><dd>{STATUS_LABEL[session.status]}</dd>
              <dt>Started by</dt><dd className="row"><Avatar user={owner} size={18} /> {owner?.name ?? "Unknown"}</dd>
              <dt>Harness</dt><dd>{data.harness?.title ?? "—"}</dd>
              <dt>Model</dt><dd>{data.harness?.model ?? "Harness default"}</dd>
              {session.workflow && <><dt>Workflow</dt><dd>{session.workflow}</dd></>}
              <dt>Created</dt><dd>{timeAgo(session.created_at)}</dd>
              {session.repos.length > 0 && <><dt>Repos</dt><dd>{session.repos.join(", ")}</dd></>}
              {session.repos.length > 0 && <><dt>Branch</dt><dd className="mono">{session.work_branch}</dd></>}
              {session.skills.length > 0 && <><dt>Skills</dt><dd>{session.skills.join(", ")}</dd></>}
            </dl>
          </div>

          {session.links.length > 0 && (
            <div className="card card-pad">
              <h3>Produced</h3>
              <div className="stack" style={{ gap: 6 }}>
                {session.links.map((l) => <a key={l.url} href={l.url} target="_blank" rel="noreferrer noopener">{l.title}</a>)}
              </div>
            </div>
          )}

          {plan && plan.length > 0 && (
            <div className="card card-pad">
              <h3>Plan</h3>
              <ul className="plan">
                {plan.map((entry, i) => (
                  <li key={i} className={entry.status}>
                    <span className="box" />
                    <span>{entry.content}</span>
                  </li>
                ))}
              </ul>
            </div>
          )}

          {children.length > 0 && (
            <div className="card">
              <div className="card-pad" style={{ paddingBottom: 0 }}><h3>Agents it started</h3></div>
              {children.map((c) => <AgentRow key={c.id} session={c} />)}
            </div>
          )}

          <div className="card card-pad">
            <h3>May do</h3>
            {session.grants.length === 0 ? (
              <p className="small muted" style={{ margin: 0 }}>Nothing outside its sandbox.</p>
            ) : (
              <div className="row-wrap">{session.grants.map((g) => <span key={g} className="chip">{g}</span>)}</div>
            )}
          </div>
        </aside>
      </div>
    </div>
  );
}

function TranscriptItem({ item }: { item: Item }) {
  const users = useStore((s) => s.users);
  switch (item.type) {
    case "user": {
      const platform = !item.userId && item.author === "Noemune";
      return (
        <div className={`msg-user ${platform ? "msg-platform" : ""}`}>
          <div className="who">
            {item.userId && <Avatar user={users[item.userId]} size={16} />}
            {item.author} · {clock(item.ts)}
          </div>
          {item.text}
        </div>
      );
    }
    case "agent":
      return <div className="msg-agent"><Markdown text={item.text} /></div>;
    case "thought":
      return (
        <details className="thought">
          <summary>Thinking</summary>
          {item.text}
        </details>
      );
    case "tool":
      return <ToolCall data={item.data} />;
    case "gate": {
      const denied = item.data.allowed === false;
      return (
        <div className={`gate ${denied ? "gate-denied" : ""}`} title={item.data.detail || undefined}>
          {denied ? "Blocked" : item.data.failed ? "Failed" : "Allowed"} · {item.data.summary}
        </div>
      );
    }
    case "note":
      return <div className={`sysline ${item.rule ? "rule" : ""}`}>{item.text}</div>;
    case "error":
      return <div className="err-block">{item.text}</div>;
  }
}

const TOOL_STATUS: Record<string, string> = { pending: "Queued", in_progress: "Running", completed: "Done", failed: "Failed" };

function ToolCall({ data }: { data: any }) {
  const failed = data.status === "failed";
  const content: any[] = Array.isArray(data.content) ? data.content : [];
  const parts: ReactNode[] = [];
  content.forEach((c, i) => {
    if (c?.type === "content" && c.content?.type === "text" && c.content.text) {
      parts.push(<pre key={i}>{c.content.text}</pre>);
    } else if (c?.type === "diff") {
      parts.push(
        <div key={i}>
          <div className="mono muted" style={{ marginBottom: 4 }}>{c.path}</div>
          <pre>
            {c.oldText ? String(c.oldText).split("\n").slice(0, 200).map((l, j) => <span key={`o${j}`} className="diff-del">- {l}</span>) : null}
            {String(c.newText ?? "").split("\n").slice(0, 400).map((l, j) => <span key={`n${j}`} className="diff-add">+ {l}</span>)}
          </pre>
        </div>,
      );
    }
  });
  const hasInput = data.rawInput && typeof data.rawInput === "object" && Object.keys(data.rawInput).length > 0;
  const expandable = parts.length > 0 || hasInput;
  return (
    <details className={`tool ${failed ? "tool-failed" : ""}`} open={failed && parts.length > 0}>
      <summary style={expandable ? undefined : { cursor: "default" }}>
        <span className="chip">{data.kind ?? "tool"}</span>
        <span className="title truncate">{data.title ?? "Tool call"}</span>
        <span className="tool-status">{TOOL_STATUS[data.status] ?? ""}</span>
      </summary>
      {expandable && (
        <div className="tool-body">
          {hasInput && <pre>{JSON.stringify(data.rawInput, null, 2)}</pre>}
          {parts}
        </div>
      )}
    </details>
  );
}
