import { useMemo, useState } from "react";
import { type Activity, isLive, type Session, type Workstream, type WorkstreamStatus } from "../lib/api";
import { useStore } from "../lib/store";
import { A, Avatar, Empty, StatusDot, timeAgo, useTick } from "../lib/ui";
import { Composer } from "../components/Composer";

const COLUMNS: { status: WorkstreamStatus; label: string; hint: string }[] = [
  { status: "scoping", label: "Scoping", hint: "Being shaped" },
  { status: "building", label: "Building", hint: "Agents at work" },
  { status: "review", label: "In review", hint: "Waiting on people" },
  { status: "done", label: "Done", hint: "" },
];

export function Board() {
  useTick();
  const [composing, setComposing] = useState(false);
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
    .filter((s) => !s.workstream_id && !s.parent_id && isLive(s))
    .sort((a, b) => b.created_at.localeCompare(a.created_at));
  const weekAgo = Date.now() - 7 * 86400_000;

  return (
    <div className="page page-wide">
      <div className="page-head">
        <div>
          <h1>Board</h1>
          <p>Everything the team and its agents are working on, as it happens.</p>
        </div>
        <button className="btn btn-primary" onClick={() => setComposing(true)}>New work</button>
      </div>

      <div className="board-layout">
        <div>
          {visible.length === 0 && loose.length === 0 ? (
            <Empty title="Nothing in flight yet">
              Describe something you want built or investigated and an agent will pick it up. Your teammates will be able to follow along here.
            </Empty>
          ) : (
            <div className="board">
              {COLUMNS.map((col) => {
                const items = visible
                  .filter((w) => w.status === col.status)
                  // Done work drops off after a week; it stays reachable from Activity.
                  .filter((w) => col.status !== "done" || new Date(w.updated_at).getTime() > weekAgo)
                  .sort((a, b) => b.updated_at.localeCompare(a.updated_at));
                return (
                  <section className="column" key={col.status}>
                    <header>
                      {col.label} <span className="n">{items.length}</span>
                    </header>
                    <div className="column-body">
                      {items.map((w) => <WorkstreamCard key={w.id} workstream={w} sessions={byWorkstream[w.id] ?? []} />)}
                    </div>
                  </section>
                );
              })}
            </div>
          )}

          {loose.length > 0 && (
            <div className="loose">
              <h2 style={{ marginBottom: 10 }}>Agents not on the board</h2>
              <div className="card">
                {loose.map((s) => <AgentRow key={s.id} session={s} />)}
              </div>
            </div>
          )}
        </div>

        <aside className="feed">
          <h2 style={{ marginBottom: 6 }}>Latest</h2>
          <Feed items={activity.slice(0, 25)} />
          {activity.length > 25 && <A href="/activity" className="small">All activity</A>}
        </aside>
      </div>

      {composing && <Composer onClose={() => setComposing(false)} />}
    </div>
  );
}

function WorkstreamCard({ workstream, sessions }: { workstream: Workstream; sessions: Session[] }) {
  const users = useStore((s) => s.users);
  const live = sessions.filter(isLive);
  const people = [...new Set([workstream.created_by, ...sessions.map((s) => s.created_by)])];
  const prs = workstream.links.filter((l) => l.kind === "pull_request");
  return (
    <A href={`/w/${workstream.id}`} className="card ws-card">
      <h3>{workstream.title}</h3>
      {live.length > 0 && (
        <div className="ws-agents">
          {live.slice(0, 4).map((s) => (
            <div className="ws-agent" key={s.id}>
              <StatusDot status={s.status} />
              <span className="truncate">
                {s.title !== workstream.title && <>{s.title} </>}
                <span className="what">{s.status === "running" ? s.headline : s.status === "idle" ? "waiting for input" : "starting…"}</span>
              </span>
            </div>
          ))}
          {live.length > 4 && <div className="small faint">+{live.length - 4} more agents</div>}
        </div>
      )}
      <div className="spread" style={{ marginTop: 8 }}>
        <div className="row-wrap">
          {prs.length > 0 && <span className="chip chip-accent">{prs.length} PR{prs.length > 1 ? "s" : ""}</span>}
          {sessions.length > 0 && live.length === 0 && <span className="chip">{sessions.length} agent{sessions.length > 1 ? "s" : ""}</span>}
          <span className="small faint">{timeAgo(workstream.updated_at)}</span>
        </div>
        <div className="row" style={{ gap: 3 }}>
          {people.slice(0, 3).map((id) => <Avatar key={id} user={users[id]} size={20} />)}
        </div>
      </div>
    </A>
  );
}

/** One line of markdown as plain text, for list rows. */
const plain = (line: string) => line.replace(/\*\*|`/g, "").replace(/^#+\s*/, "");

export function AgentRow({ session, child }: { session: Session; child?: boolean }) {
  const users = useStore((s) => s.users);
  const line =
    session.status === "running" ? session.headline :
    session.status === "failed" ? session.error ?? "" :
    plain(session.summary.split("\n").find((l) => l.trim()) ?? session.headline);
  return (
    <A href={`/s/${session.id}`} className={`agent-row ${child ? "agent-child" : ""}`}>
      <StatusDot status={session.status} />
      <div className="grow">
        <div className="row">
          <strong className="truncate">{session.title}</strong>
          {session.role === "conductor" && <span className="chip chip-accent">Conductor</span>}
        </div>
        {line && <div className="small muted truncate">{line}</div>}
      </div>
      {session.links.filter((l) => l.kind === "pull_request").map((l) => <span key={l.url} className="chip chip-accent">PR</span>)}
      <span className="small faint">{timeAgo(session.last_activity_at)}</span>
      <Avatar user={users[session.created_by]} size={20} />
    </A>
  );
}

export function Feed({ items }: { items: Activity[] }) {
  if (items.length === 0) return <p className="small muted">Activity from people and agents will show up here.</p>;
  return (
    <div className="feed-list">
      {items.map((item) => {
        const href = item.session_id ? `/s/${item.session_id}` : item.workstream_id ? `/w/${item.workstream_id}` : null;
        return (
          <div className="feed-item" key={item.id}>
            <div className="grow">
              {href ? <A href={href}>{item.summary}</A> : item.summary}
              {item.data?.url && (
                <div><a className="small" href={item.data.url} target="_blank" rel="noreferrer noopener">Open pull request</a></div>
              )}
            </div>
            <span className="when">{timeAgo(item.ts)}</span>
          </div>
        );
      })}
    </div>
  );
}
