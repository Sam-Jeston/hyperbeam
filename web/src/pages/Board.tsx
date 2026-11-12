import { GitPullRequest, Plus, Sprout } from "lucide-react";
import { useMemo } from "react";
import { Button } from "../components/ui/button";
import { Tooltip } from "../components/ui/menu";
import { Badge, Card, EmptyState, Panel } from "../components/ui/surface";
import { type Activity, isLive, type Session, type Workstream, type WorkstreamStatus } from "../lib/api";
import { useStore } from "../lib/store";
import { A, Avatar, AvatarStack, dayLabel, openComposer, Page, PageHeader, SectionTitle, StatusDot, Time, useDocumentTitle, useTick } from "../lib/ui";
import { cn } from "../lib/utils";

export const STAGES: { status: WorkstreamStatus; label: string; hint: string; dot: string }[] = [
  { status: "scoping", label: "Scoping", hint: "Being shaped before anyone builds", dot: "bg-idle" },
  { status: "building", label: "Building", hint: "Agents are at work", dot: "bg-run" },
  { status: "review", label: "In review", hint: "Waiting on people", dot: "bg-wait" },
  { status: "done", label: "Done", hint: "Finished in the last seven days", dot: "bg-primary" },
];

export function Board() {
  useTick();
  useDocumentTitle("Board");
  const workstreams = useStore((s) => s.workstreams);
  const sessions = useStore((s) => s.sessions);
  const activity = useStore((s) => s.activity);

  const byWorkstream = useMemo(() => {
    const map: Record<string, Session[]> = {};
    for (const session of Object.values(sessions)) {
      if (session.workstream_id) (map[session.workstream_id] ??= []).push(session);
    }
    return map;
  }, [sessions]);

  const visible = Object.values(workstreams).filter((w) => w.status !== "archived");
  const loose = Object.values(sessions)
    .filter((s) => !s.workstream_id && !s.parent_id && (isLive(s) || s.question))
    .sort((a, b) => b.created_at.localeCompare(a.created_at));
  const weekAgo = Date.now() - 7 * 86400_000;
  const empty = visible.length === 0 && loose.length === 0;

  return (
    <Page width="wide">
      <PageHeader
        title="Board"
        description="Everything the team and its agents are working on, as it happens."
        actions={<Button variant="primary" onClick={() => openComposer()}><Plus /> New work</Button>}
      />

      <div className="grid items-start gap-6 min-[1680px]:grid-cols-[minmax(0,1fr)_320px]">
        <div className="min-w-0">
          {empty ? (
            <EmptyState
              icon={<Sprout />}
              title="Nothing growing yet"
              action={<Button variant="primary" onClick={() => openComposer()}><Plus /> Start new work</Button>}
            >
              Describe something you want built or investigated. An agent starts on it in its own sandbox, and your teammates can follow along here.
            </EmptyState>
          ) : (
            <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
              {STAGES.map((stage) => {
                const items = visible
                  .filter((w) => w.status === stage.status)
                  // Done work drops off after a week; it stays reachable from Activity.
                  .filter((w) => stage.status !== "done" || new Date(w.updated_at).getTime() > weekAgo)
                  .sort((a, b) => b.updated_at.localeCompare(a.updated_at));
                return (
                  <section key={stage.status} className="flex min-w-0 flex-col rounded-xl bg-surface-2/45 p-2" aria-label={stage.label}>
                    <header className="flex items-center gap-2 px-1.5 pt-1 pb-2.5">
                      <span className={cn("size-2 rounded-full", stage.dot)} />
                      <Tooltip content={stage.hint}>
                        <h2 className="text-[13px] font-semibold">{stage.label}</h2>
                      </Tooltip>
                      <span className="text-[12.5px] tabular-nums text-faint">{items.length}</span>
                    </header>
                    <div className="grid gap-2">
                      {items.map((w) => <WorkstreamCard key={w.id} workstream={w} sessions={byWorkstream[w.id] ?? []} />)}
                      {items.length === 0 && (
                        <div className="rounded-lg border border-dashed border-border px-3 py-5 text-center text-[12.5px] text-faint">{stage.hint}</div>
                      )}
                    </div>
                  </section>
                );
              })}
            </div>
          )}

          {loose.length > 0 && (
            <section className="mt-8">
              <SectionTitle description="Started without a workstream. Add one to track it on the board.">Agents not on the board</SectionTitle>
              <Card className="divide-y divide-border overflow-hidden">
                {loose.map((s) => <AgentRow key={s.id} session={s} />)}
              </Card>
            </section>
          )}
        </div>

        <Panel title="Latest" className="min-[1680px]:sticky min-[1680px]:top-6 min-[1680px]:max-w-none max-w-[760px]" action={activity.length > 0 ? <A href="/activity" className="text-[12.5px] text-muted hover:text-foreground">See all</A> : undefined}>
          <Feed items={activity.slice(0, 20)} />
        </Panel>
      </div>
    </Page>
  );
}

function WorkstreamCard({ workstream, sessions }: { workstream: Workstream; sessions: Session[] }) {
  const users = useStore((s) => s.users);
  const live = sessions.filter(isLive);
  const people = [...new Set([workstream.created_by, ...sessions.map((s) => s.created_by)])];
  const prs = workstream.links.filter((l) => l.kind === "pull_request");
  const needsYou = live.some((s) => s.status === "idle");
  // Questions put to people; one a worker put to its conductor is the conductor's to handle.
  const asking = sessions.filter((s) => s.question?.to === "team").length;
  return (
    <A href={`/w/${workstream.id}`} className="block rounded-lg focus-visible:outline-2 focus-visible:outline-ring">
      <Card interactive className="grid gap-2.5 p-3 shadow-[0_1px_0_hsl(var(--shadow-color)/0.06)]">
        <h3 className="text-[13.5px] font-semibold leading-snug [overflow-wrap:anywhere]">{workstream.title}</h3>
        {live.length > 0 && (
          <ul className="grid gap-1.5">
            {live.slice(0, 3).map((s) => (
              <li key={s.id} className="flex min-w-0 items-center gap-2 text-[12.5px]">
                <StatusDot status={s.status} />
                <span className="truncate text-muted">
                  {s.title !== workstream.title && <span className="text-foreground/90">{s.title}</span>}
                  {/* Idle is shown once, as the card's "Needs a reply" badge. */}
                  {s.status !== "idle" && (
                    <>
                      {s.title !== workstream.title && " · "}
                      {s.status === "running" ? s.headline || "Working" : "Starting"}
                    </>
                  )}
                  {s.status === "idle" && s.title === workstream.title && "Waiting for a reply"}
                </span>
              </li>
            ))}
            {live.length > 3 && <li className="pl-4 text-[12px] text-faint">and {live.length - 3} more</li>}
          </ul>
        )}
        <div className="flex items-center justify-between gap-2">
          <div className="flex min-w-0 flex-wrap items-center gap-1.5 text-[12px] text-faint">
            {asking > 0 ? <Badge tone="wait">{asking > 1 ? `${asking} questions` : "Has a question"}</Badge> : needsYou && <Badge tone="idle">Needs a reply</Badge>}
            {prs.length > 0 && <Badge tone="primary"><GitPullRequest /> {prs.length}</Badge>}
            {sessions.length > 0 && live.length === 0 && <span>{sessions.length} agent{sessions.length > 1 ? "s" : ""}</span>}
            <Time iso={workstream.updated_at} />
          </div>
          <AvatarStack users={people.map((id) => users[id])} size={20} />
        </div>
      </Card>
    </A>
  );
}

/** One line of markdown as plain text, for list rows. */
const plain = (line: string) => line.replace(/\*\*|`/g, "").replace(/^#+\s*/, "");

export function AgentRow({ session, child }: { session: Session; child?: boolean }) {
  const users = useStore((s) => s.users);
  const line =
    session.question ? `Asks: ${session.question.text}` :
    session.status === "running" ? session.headline :
    session.status === "failed" ? session.error ?? "" :
    plain(session.summary.split("\n").find((l) => l.trim()) ?? session.headline);
  const prs = session.links.filter((l) => l.kind === "pull_request");
  return (
    <A
      href={`/s/${session.id}`}
      className={cn("flex items-center gap-3 px-4 py-2.5 transition-colors hover:bg-surface-2/70 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring", child && "pl-10")}
    >
      <StatusDot status={session.status} />
      <div className="grid min-w-0 flex-1 gap-0.5">
        <div className="flex min-w-0 items-center gap-2">
          <span className="truncate text-[13.5px] font-medium">{session.title}</span>
          {session.role === "conductor" && <Badge tone="primary">Conductor</Badge>}
        </div>
        {line && <div className={cn("truncate text-[12.5px]", session.question ? "text-wait" : session.status === "failed" ? "text-fail" : "text-muted")}>{line}</div>}
      </div>
      {prs.length > 0 && <Badge tone="primary"><GitPullRequest /> PR</Badge>}
      <Time iso={session.last_activity_at} className="hidden text-[12px] text-faint sm:inline" />
      <Avatar user={users[session.created_by]} size={22} />
    </A>
  );
}

/** Recent activity, grouped by day. */
export function Feed({ items, grouped = false }: { items: Activity[]; grouped?: boolean }) {
  const users = useStore((s) => s.users);
  if (items.length === 0) return <p className="py-2 text-[13px] text-muted">Activity from people and agents will show up here.</p>;
  const groups: { day: string; items: Activity[] }[] = [];
  for (const item of items) {
    const day = grouped ? dayLabel(item.ts) : "";
    if (groups.at(-1)?.day !== day) groups.push({ day, items: [] });
    groups.at(-1)!.items.push(item);
  }
  return (
    <div className="grid gap-4">
      {groups.map((group) => (
        <div key={group.day || "all"}>
          {group.day && <div className="mb-1 text-[11.5px] font-semibold uppercase tracking-[0.07em] text-faint">{group.day}</div>}
          <ol className="relative grid">
            {group.items.map((item) => {
              const href = item.session_id ? `/s/${item.session_id}` : item.workstream_id ? `/w/${item.workstream_id}` : null;
              const actor = item.user_id ? users[item.user_id] : undefined;
              return (
                <li key={item.id} className="relative flex gap-3 py-2 pl-0.5 text-[13px]">
                  <div className="relative flex w-5 shrink-0 justify-center pt-0.5">
                    {actor ? <Avatar user={actor} size={18} className="ring-0" /> : <span className="mt-1.5 size-2 rounded-full bg-border-strong" />}
                  </div>
                  <div className="min-w-0 flex-1 leading-snug [overflow-wrap:anywhere]">
                    {href ? <A href={href} className="hover:text-primary">{item.summary}</A> : item.summary}
                    {item.data?.url && (
                      <a className="mt-1 flex items-center gap-1 text-[12.5px] text-primary hover:underline" href={item.data.url} target="_blank" rel="noreferrer noopener">
                        <GitPullRequest className="size-3.5" /> Open pull request
                      </a>
                    )}
                  </div>
                  <Time iso={item.ts} className="pt-px text-[12px] text-faint" />
                </li>
              );
            })}
          </ol>
        </div>
      ))}
    </div>
  );
}
