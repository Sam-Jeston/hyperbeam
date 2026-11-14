import * as Collapsible from "@radix-ui/react-collapsible";
import {
  ArrowDown, ChevronRight, CircleCheck, CircleStop, CircleX, ExternalLink, FilePen, FileText, FolderGit2, Globe, Hand, Lightbulb,
  Loader2, MessageCircleQuestion, Search, SendHorizontal, ShieldCheck, ShieldX, Terminal, Trash2, Wrench,
} from "lucide-react";
import { type ReactNode, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { BrandMark } from "../components/ui/brand";
import { Button } from "../components/ui/button";
import { confirm } from "../components/ui/dialog";
import { Kbd } from "../components/ui/input";
import { Badge, Card, KeyValues, Notice, Panel, Skeleton } from "../components/ui/surface";
import { api, isLive, type Question, type Session, type SessionEvent } from "../lib/api";
import { markRead, useStore } from "../lib/store";
import { A, Avatar, clock, ErrorNote, Ext, Markdown, Page, PageHeader, StatusBadge, Time, useAction, useDocumentTitle, useLoad, useTick } from "../lib/ui";
import { cn } from "../lib/utils";
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
  | { type: "question"; key: number; id: string; text: string; options: string[]; to: string }
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
      case "question":
        items.push({ type: "question", key: seq, id: data.id, text: data.text ?? "", options: Array.isArray(data.options) ? data.options : [], to: data.to ?? "team" });
        break;
      case "system":
        items.push({ type: "note", key: seq, text: data.text ?? "" });
        break;
      case "status":
        if (data.status === "cancelled") items.push({ type: "note", key: seq, text: "Stopped", rule: true });
        else if (data.status === "completed") items.push({ type: "note", key: seq, text: "Asleep · the sandbox shut down", rule: true });
        else if (data.status === "failed") items.push({ type: "note", key: seq, text: "Failed", rule: true });
        break;
      case "turn_end":
        if (data.stop_reason === "cancelled") items.push({ type: "note", key: seq, text: "Interrupted", rule: true });
        else if (data.stop_reason === "refusal") items.push({ type: "note", key: seq, text: "The agent declined to continue", rule: true });
        else if (data.stop_reason === "max_tokens" || data.stop_reason === "max_turn_requests") items.push({ type: "note", key: seq, text: "The agent hit its limit for one turn. Send a message to continue.", rule: true });
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
  const action = useAction();
  const bottom = useRef<HTMLDivElement>(null);
  const stuck = useRef(true);
  const [behind, setBehind] = useState(false);
  const session = liveSession ?? data?.session;
  useDocumentTitle(session?.title);

  // Opening an agent's page is reading what it wanted to tell you.
  useEffect(() => {
    const seen = () => {
      if (document.visibilityState === "visible") markRead({ session_id: id });
    };
    seen();
    document.addEventListener("visibilitychange", seen);
    return () => document.removeEventListener("visibilitychange", seen);
  }, [id]);

  // Follow the transcript while the reader is at the bottom of it.
  useEffect(() => {
    const scroller = document.getElementById("main-scroll");
    if (!scroller) return;
    const onScroll = () => {
      stuck.current = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 160;
      if (stuck.current) setBehind(false);
    };
    scroller.addEventListener("scroll", onScroll, { passive: true });
    return () => scroller.removeEventListener("scroll", onScroll);
  }, []);
  useLayoutEffect(() => {
    if (stuck.current) bottom.current?.scrollIntoView({ block: "end" });
    else if (caughtUp) setBehind(true);
  }, [events.length, caughtUp, !!data]);

  if (error) return <Page><ErrorNote title="Could not open this session" error={error} /></Page>;
  if (!data || !session) return <SessionSkeleton />;

  const children = Object.values(allSessions).filter((s) => s.parent_id === id).sort((a, b) => a.created_at.localeCompare(b.created_at));
  const owner = users[session.created_by];
  const live = isLive(session);
  const breadcrumbs = [
    { label: "Board", href: "/" },
    ...(data.workstream ? [{ label: data.workstream.title, href: `/w/${data.workstream.id}` }] : [{ label: "Agents", href: "/agents" }]),
    ...(data.parent ? [{ label: data.parent.title, href: `/s/${data.parent.id}` }] : []),
  ];

  const stop = async () => {
    const ok = await confirm({
      title: "Stop this agent?",
      description: "Its sandbox shuts down and any unpushed work in it is lost. You can still read the transcript, and send a message later to start it again in a fresh sandbox.",
      confirmLabel: "Stop agent",
      destructive: true,
    });
    if (ok) void action.run(() => api.post(`/sessions/${id}/stop`), { success: "Agent stopped", toastError: true });
  };

  return (
    <Page className="pb-0">
      <PageHeader
        breadcrumbs={breadcrumbs}
        title={session.title}
        meta={
          <>
            <StatusBadge session={session} />
            {session.role === "conductor" && <Badge tone="primary">Conductor</Badge>}
            {session.status === "running" && session.headline && <span className="max-w-[60ch] truncate">{session.headline}</span>}
          </>
        }
        actions={
          data.can_drive && live && (
            <>
              {session.status === "running" && (
                <Button onClick={() => void action.run(() => api.post(`/sessions/${id}/cancel`), { success: "Interrupted. The agent will wait for your next message.", toastError: true })}>
                  <Hand /> Interrupt
                </Button>
              )}
              <Button variant="danger" onClick={() => void stop()}><CircleStop /> Stop</Button>
            </>
          )
        }
      />

      <div className="grid items-start gap-6 lg:grid-cols-[minmax(0,1fr)_300px]">
        {/* Fills the viewport so the reply box sits at the bottom even when the transcript is short. */}
        <div className="flex min-h-[calc(100dvh-180px)] min-w-0 flex-col">
          <div className="grid flex-1 content-start gap-3.5 pb-4" aria-live="polite" aria-busy={!caughtUp}>
            {!caughtUp && items.length === 0 && <TranscriptSkeleton />}
            {items.map((item) =>
              item.type === "question" ? (
                <QuestionCard
                  key={item.key}
                  sessionId={id}
                  question={item}
                  open={session.question?.id === item.id}
                  canAnswer={data.can_drive}
                  onAnswered={() => (stuck.current = true)}
                />
              ) : (
                <TranscriptItem key={item.key} item={item} />
              ),
            )}
            {session.status === "running" && (
              <div className="flex items-center gap-2 text-[13px] text-muted">
                <Loader2 className="size-3.5 animate-spin text-run" /> Working
              </div>
            )}
            {(session.status === "starting" || session.status === "pending") && (
              <div className="flex items-center gap-2 text-[13px] text-muted">
                <Loader2 className="size-3.5 animate-spin text-wait" /> {session.status === "pending" ? "Waiting for a free sandbox" : "Preparing the sandbox"}
              </div>
            )}
            <div ref={bottom} />
          </div>

          <div className="sticky bottom-0 z-10 -mx-1 bg-gradient-to-t from-background from-70% to-transparent px-1 pt-6 pb-4">
            {behind && (
              <div className="pointer-events-none absolute inset-x-0 -top-2 flex justify-center">
                <Button
                  size="sm"
                  className="pointer-events-auto rounded-full shadow-pop"
                  onClick={() => {
                    stuck.current = true;
                    setBehind(false);
                    bottom.current?.scrollIntoView({ block: "end", behavior: "smooth" });
                  }}
                >
                  <ArrowDown /> New activity
                </Button>
              </div>
            )}
            {data.can_drive ? (
              <MessageComposer
                sessionId={id}
                question={session.question}
                placeholder={
                  session.question ? "Answer in your own words" :
                  live
                    ? session.status === "running" ? "Queue a follow-up for when this turn ends" : "Reply to the agent"
                    : "Send a message to wake this agent in a fresh sandbox"
                }
                onSent={() => (stuck.current = true)}
              />
            ) : (
              <Card className="px-4 py-3 text-[13px] text-muted">
                You are watching. This agent holds permissions you do not, so only {owner?.name ?? "its owner"} or an admin can direct it.
                {data.workstream && <> You can comment on <A href={`/w/${data.workstream.id}`} className="text-primary hover:underline">its workstream</A>.</>}
              </Card>
            )}
          </div>
        </div>

        <aside className="grid gap-4 pb-8 lg:sticky lg:top-6">
          {plan && plan.length > 0 && <PlanPanel plan={plan} />}
          <Panel title="About">
            <KeyValues
              rows={[
                ["Started by", <span className="flex items-center gap-1.5"><Avatar user={owner} size={18} className="ring-0" /> {owner?.name ?? "Unknown"}</span>],
                ["Harness", data.harness?.title ?? "None"],
                ["Model", data.harness?.model ?? <span className="text-muted">Harness default</span>],
                ...(session.workflow ? [["Workflow", session.workflow] as [ReactNode, ReactNode]] : []),
                ["Created", <Time iso={session.created_at} />],
                ...(session.repos.length > 0
                  ? [
                      ["Repos", <div className="grid gap-0.5">{session.repos.map((r) => <span key={r} className="font-mono text-[12px]">{r}</span>)}</div>] as [ReactNode, ReactNode],
                      ["Branch", <span className="font-mono text-[12px]">{session.work_branch}</span>] as [ReactNode, ReactNode],
                    ]
                  : []),
                ...(session.skills.length > 0 ? [["Skills", session.skills.join(", ")] as [ReactNode, ReactNode]] : []),
              ]}
            />
          </Panel>

          {session.links.length > 0 && (
            <Panel title="Produced">
              <ul className="grid gap-1.5 text-[13px]">
                {session.links.map((l) => (
                  <li key={l.url}><Ext href={l.url} className="flex items-start gap-1.5"><ExternalLink className="mt-0.5 size-3.5 shrink-0" />{l.title}</Ext></li>
                ))}
              </ul>
            </Panel>
          )}

          {children.length > 0 && (
            <Panel title={`Agents it started · ${children.length}`} flush>
              <div className="divide-y divide-border border-t border-border">{children.map((c) => <AgentRow key={c.id} session={c} />)}</div>
            </Panel>
          )}

          <Panel title="May do">
            {session.grants.length === 0 ? (
              <p className="text-[13px] text-muted">Nothing outside its sandbox.</p>
            ) : (
              <div className="flex flex-wrap gap-1.5">{session.grants.map((g) => <Badge key={g} tone="outline" mono>{g}</Badge>)}</div>
            )}
          </Panel>
        </aside>
      </div>
    </Page>
  );
}

/** A question the agent asked. While it is open, its options are one-click answers. */
function QuestionCard(props: {
  sessionId: string;
  question: { id: string; text: string; options: string[]; to: string };
  open: boolean;
  canAnswer: boolean;
  onAnswered: () => void;
}) {
  const { question, open } = props;
  const action = useAction();
  const answer = (text: string) =>
    void action.run(async () => {
      await api.post(`/sessions/${props.sessionId}/answer`, { text, question_id: question.id });
      props.onAnswered();
    });
  return (
    <div className={cn("grid gap-2.5 rounded-lg border px-3.5 py-3", open ? "border-wait/40 bg-wait/8" : "border-border bg-surface-2/50")}>
      <div className={cn("flex items-center gap-1.5 text-[12px] font-medium", open ? "text-wait" : "text-muted")}>
        <MessageCircleQuestion className="size-3.5" />
        {question.to === "conductor" ? "Question for its conductor" : "Question"}
        {!open && <span className="font-normal text-faint">· no longer open</span>}
      </div>
      <Markdown text={question.text} className="max-w-[78ch] text-[14px]" />
      {open && props.canAnswer && question.options.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {question.options.map((option) => (
            <Button key={option} size="sm" disabled={action.busy} onClick={() => answer(option)} className="h-auto min-h-7 max-w-full py-1 text-left whitespace-normal">
              {option}
            </Button>
          ))}
        </div>
      )}
      {!open && question.options.length > 0 && <div className="text-[12.5px] text-faint">Offered: {question.options.join(" · ")}</div>}
      {open && props.canAnswer && <div className="text-[12.5px] text-muted">{question.options.length > 0 ? "Pick one, or answer in your own words below." : "Answer below."}</div>}
      <ErrorNote error={action.error} />
    </div>
  );
}

function MessageComposer({ sessionId, question, placeholder, onSent }: { sessionId: string; question?: Question | null; placeholder: string; onSent: () => void }) {
  const [text, setText] = useState("");
  const action = useAction();
  const ref = useRef<HTMLTextAreaElement>(null);
  // Grow with the message, up to a limit.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 240)}px`;
  }, [text]);
  const send = () => {
    const message = text.trim();
    if (!message || action.busy) return;
    void action.run(async () => {
      // While a question is open, what is typed here is the answer to it.
      if (question) await api.post(`/sessions/${sessionId}/answer`, { text: message, question_id: question.id });
      else await api.post(`/sessions/${sessionId}/messages`, { text: message });
      setText("");
      onSent();
    });
  };
  return (
    <div className="grid gap-2">
      <div className="rounded-xl border border-border-strong bg-surface p-2 shadow-pop transition-[border-color,box-shadow] focus-within:border-primary/70 focus-within:ring-3 focus-within:ring-primary/15">
        <textarea
          ref={ref}
          rows={1}
          value={text}
          aria-label="Message"
          placeholder={placeholder}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              send();
            }
          }}
          className="block max-h-60 min-h-11 w-full resize-none bg-transparent px-2 py-1.5 text-[14px] leading-relaxed outline-none placeholder:text-faint"
        />
        <div className="flex items-center justify-between gap-2 pl-2">
          <span className="hidden items-center gap-1 text-[12px] text-faint sm:flex">
            <Kbd>Enter</Kbd> to send · <Kbd>Shift</Kbd><Kbd>Enter</Kbd> for a new line
          </span>
          <Button variant="primary" size="sm" className="ml-auto" disabled={!text.trim()} loading={action.busy} onClick={send}>
            {!action.busy && <SendHorizontal />} {question ? "Answer" : "Send"}
          </Button>
        </div>
      </div>
      <ErrorNote error={action.error} />
    </div>
  );
}

function PlanPanel({ plan }: { plan: any[] }) {
  const done = plan.filter((p) => p.status === "completed").length;
  return (
    <Panel title="Plan" action={<span className="text-[12px] tabular-nums text-muted">{done} of {plan.length}</span>}>
      <div className="mb-3 h-1 overflow-hidden rounded-full bg-surface-2">
        <div className="h-full rounded-full bg-run transition-[width] duration-500" style={{ width: `${(done / plan.length) * 100}%` }} />
      </div>
      <ul className="grid gap-2 text-[13px]">
        {plan.map((entry, i) => (
          <li key={i} className="flex items-start gap-2">
            {entry.status === "completed" ? (
              <CircleCheck className="mt-0.5 size-4 shrink-0 text-run" />
            ) : entry.status === "in_progress" ? (
              <Loader2 className="mt-0.5 size-4 shrink-0 animate-spin text-primary" />
            ) : (
              <span className="mt-0.5 size-4 shrink-0 rounded-full border-[1.5px] border-border-strong" />
            )}
            <span className={cn("leading-snug", entry.status === "completed" && "text-muted line-through decoration-faint", entry.status === "in_progress" && "font-medium")}>
              {entry.content}
            </span>
          </li>
        ))}
      </ul>
    </Panel>
  );
}

function TranscriptItem({ item }: { item: Item }) {
  const users = useStore((s) => s.users);
  switch (item.type) {
    case "user": {
      if (!item.userId && item.author === "Hyperbeam") {
        return (
          <div className="flex gap-3 rounded-lg border border-border bg-surface-2/50 px-3.5 py-2.5 text-[13px]">
            <BrandMark size={18} className="mt-0.5" />
            <div className="min-w-0 flex-1">
              <div className="mb-0.5 text-[12px] text-muted">Hyperbeam · {clock(item.ts)}</div>
              <div className="whitespace-pre-wrap [overflow-wrap:anywhere]">{item.text}</div>
            </div>
          </div>
        );
      }
      return (
        <div className="flex flex-col items-end gap-1 pl-[12%]">
          <div className="flex items-center gap-1.5 text-[12px] text-muted">
            {item.userId && <Avatar user={users[item.userId]} size={16} className="ring-0" />}
            {item.author} · {clock(item.ts)}
          </div>
          <div className="max-w-full rounded-2xl rounded-tr-md bg-accent px-3.5 py-2 text-[14px] leading-relaxed text-foreground whitespace-pre-wrap [overflow-wrap:anywhere]">
            {item.text}
          </div>
        </div>
      );
    }
    case "agent":
      return <Markdown text={item.text} className="max-w-[78ch] text-[14px]" />;
    case "thought":
      return (
        <Collapsible.Root className="group">
          <Collapsible.Trigger className="flex items-center gap-1.5 rounded-sm text-[12.5px] text-muted hover:text-foreground">
            <Lightbulb className="size-3.5" /> Thinking
            <ChevronRight className="size-3.5 transition-transform group-data-[state=open]:rotate-90" />
          </Collapsible.Trigger>
          <Collapsible.Content className="mt-1.5 max-w-[78ch] border-l-2 border-border-strong pl-3 text-[13px] leading-relaxed text-muted whitespace-pre-wrap">
            {item.text}
          </Collapsible.Content>
        </Collapsible.Root>
      );
    case "tool":
      return <ToolCall data={item.data} />;
    case "gate": {
      const denied = item.data.allowed === false;
      const failed = !denied && item.data.failed;
      return (
        <div
          title={item.data.detail || undefined}
          className={cn(
            "flex w-fit max-w-full items-center gap-1.5 rounded-md border px-2.5 py-1 text-[12.5px]",
            denied ? "border-fail/30 bg-fail/8 text-fail" : failed ? "border-wait/30 bg-wait/8 text-wait" : "border-border bg-surface-2/60 text-muted",
          )}
        >
          {denied ? <ShieldX className="size-3.5 shrink-0" /> : <ShieldCheck className={cn("size-3.5 shrink-0", !failed && "text-run")} />}
          <span className="font-medium">{denied ? "Blocked" : failed ? "Failed" : "Allowed"}</span>
          <span className="truncate">{item.data.summary}</span>
        </div>
      );
    }
    case "note":
      return item.rule ? (
        <div className="flex items-center gap-3 py-1 text-[12px] font-medium text-faint">
          <span className="h-px flex-1 bg-border" />
          {item.text}
          <span className="h-px flex-1 bg-border" />
        </div>
      ) : (
        <div className="text-[12.5px] text-faint">{item.text}</div>
      );
    case "error":
      return <Notice tone="error">{item.text}</Notice>;
  }
}

const TOOL_ICON: Record<string, ReactNode> = {
  read: <FileText />,
  edit: <FilePen />,
  delete: <Trash2 />,
  move: <FolderGit2 />,
  search: <Search />,
  execute: <Terminal />,
  think: <Lightbulb />,
  fetch: <Globe />,
};

function ToolCall({ data }: { data: any }) {
  const failed = data.status === "failed";
  const content: any[] = Array.isArray(data.content) ? data.content : [];
  const parts: ReactNode[] = [];
  let added = 0;
  let removed = 0;
  content.forEach((c, i) => {
    if (c?.type === "content" && c.content?.type === "text" && c.content.text) {
      parts.push(<pre key={i} className="max-h-80 overflow-auto rounded-md bg-background/70 p-2.5 font-mono text-[12px] leading-relaxed whitespace-pre-wrap [overflow-wrap:anywhere]">{c.content.text}</pre>);
    } else if (c?.type === "diff") {
      const oldLines = c.oldText ? String(c.oldText).split("\n") : [];
      const newLines = String(c.newText ?? "").split("\n");
      added += newLines.length;
      removed += oldLines.length;
      parts.push(
        <div key={i} className="overflow-hidden rounded-md border border-border">
          <div className="border-b border-border bg-surface-2/60 px-2.5 py-1 font-mono text-[11.5px] text-muted">{c.path}</div>
          <pre className="max-h-96 overflow-auto bg-background/70 py-1 font-mono text-[12px] leading-relaxed">
            {oldLines.slice(0, 200).map((l, j) => <span key={`o${j}`} className="block bg-fail/10 px-2.5 text-fail/90"><span className="select-none opacity-60">- </span>{l}</span>)}
            {newLines.slice(0, 400).map((l, j) => <span key={`n${j}`} className="block bg-run/10 px-2.5"><span className="select-none text-run opacity-70">+ </span>{l}</span>)}
          </pre>
        </div>,
      );
    }
  });
  const hasInput = data.rawInput && typeof data.rawInput === "object" && Object.keys(data.rawInput).length > 0;
  const expandable = parts.length > 0 || hasInput;
  const [open, setOpen] = useState(failed && parts.length > 0);
  const status =
    data.status === "completed" ? <CircleCheck className="size-4 text-run" aria-label="Done" /> :
    failed ? <CircleX className="size-4 text-fail" aria-label="Failed" /> :
    data.status === "in_progress" ? <Loader2 className="size-4 animate-spin text-muted" aria-label="Running" /> :
    <span className="text-[11.5px] text-faint">Queued</span>;

  return (
    <Collapsible.Root open={open} onOpenChange={setOpen} className={cn("rounded-lg border bg-surface text-[13px]", failed ? "border-fail/35" : "border-border")}>
      <Collapsible.Trigger
        disabled={!expandable}
        className="flex w-full min-w-0 items-center gap-2.5 rounded-lg px-3 py-2 text-left enabled:hover:bg-surface-2/50 disabled:cursor-default [&>svg:first-child]:size-4 [&>svg:first-child]:shrink-0 [&>svg:first-child]:text-faint"
      >
        {TOOL_ICON[data.kind] ?? <Wrench />}
        <span className="min-w-0 flex-1 truncate font-mono text-[12.5px]">{data.title ?? "Tool call"}</span>
        {(added > 0 || removed > 0) && (
          <span className="font-mono text-[11.5px] tabular-nums"><span className="text-run">+{added}</span> <span className="text-fail">−{removed}</span></span>
        )}
        {status}
        {expandable && <ChevronRight className={cn("size-4 shrink-0 text-faint transition-transform", open && "rotate-90")} />}
      </Collapsible.Trigger>
      {expandable && (
        <Collapsible.Content className="grid gap-2 border-t border-border px-3 py-2.5">
          {hasInput && <pre className="max-h-60 overflow-auto rounded-md bg-background/70 p-2.5 font-mono text-[12px] leading-relaxed text-muted whitespace-pre-wrap [overflow-wrap:anywhere]">{JSON.stringify(data.rawInput, null, 2)}</pre>}
          {parts}
        </Collapsible.Content>
      )}
    </Collapsible.Root>
  );
}

function TranscriptSkeleton() {
  return (
    <div className="grid gap-4">
      <div className="flex justify-end"><Skeleton className="h-12 w-2/3 rounded-2xl" /></div>
      <Skeleton className="h-4 w-4/5" />
      <Skeleton className="h-4 w-3/5" />
      <Skeleton className="h-9 w-full rounded-lg" />
      <Skeleton className="h-9 w-full rounded-lg" />
    </div>
  );
}

function SessionSkeleton() {
  return (
    <Page>
      <Skeleton className="mb-3 h-3.5 w-40" />
      <Skeleton className="mb-3 h-7 w-96 max-w-full" />
      <Skeleton className="mb-8 h-5 w-48" />
      <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_300px]">
        <TranscriptSkeleton />
        <div className="grid gap-4"><Skeleton className="h-40 rounded-lg" /><Skeleton className="h-24 rounded-lg" /></div>
      </div>
    </Page>
  );
}
