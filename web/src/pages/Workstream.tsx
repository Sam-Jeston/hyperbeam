import { useEffect, useMemo, useState } from "react";
import { api, type Session, type Workstream, type WorkstreamStatus } from "../lib/api";
import { useStore } from "../lib/store";
import { A, Avatar, Empty, ErrorNote, Markdown, Modal, timeAgo, useAction, useLoad, useTick } from "../lib/ui";
import { Composer } from "../components/Composer";
import { AgentRow, Feed } from "./Board";

const STATUSES: { value: WorkstreamStatus; label: string }[] = [
  { value: "scoping", label: "Scoping" },
  { value: "building", label: "Building" },
  { value: "review", label: "In review" },
  { value: "done", label: "Done" },
  { value: "archived", label: "Archived" },
];

interface Comment {
  id: string;
  user_id: string;
  author: string;
  body: string;
  created_at: string;
}

export function WorkstreamPage({ id }: { id: string }) {
  useTick();
  const users = useStore((s) => s.users);
  const allSessions = useStore((s) => s.sessions);
  const activity = useStore((s) => s.activity);
  const live = useStore((s) => s.workstreams[id]);
  const { data, error, reload } = useLoad<{ workstream: Workstream; sessions: Session[]; comments: Comment[] }>(() => api.get(`/workstreams/${id}`), [id]);
  const [composing, setComposing] = useState(false);
  const [editing, setEditing] = useState(false);
  const [comment, setComment] = useState("");
  const action = useAction();

  useEffect(() => {
    const onChange = (e: Event) => (e as CustomEvent).detail === id && reload();
    window.addEventListener("noemune:workstream", onChange);
    return () => window.removeEventListener("noemune:workstream", onChange);
  }, [id]);

  const sessions = useMemo(
    () => Object.values(allSessions).filter((s) => s.workstream_id === id).sort((a, b) => a.created_at.localeCompare(b.created_at)),
    [allSessions, id],
  );

  if (error) return <div className="page"><ErrorNote error={error} /></div>;
  if (!data) return null;
  const workstream = live ?? data.workstream;
  const roots = sessions.filter((s) => !s.parent_id || !sessions.some((p) => p.id === s.parent_id));
  const childrenOf = (parent: string) => sessions.filter((s) => s.parent_id === parent);

  const setStatus = (status: string) => action.run(() => api.patch(`/workstreams/${id}`, { status }));
  const postComment = () =>
    action.run(async () => {
      await api.post(`/workstreams/${id}/comments`, { body: comment });
      setComment("");
      reload();
    });

  return (
    <div className="page">
      <div className="small muted" style={{ marginBottom: 6 }}><A href="/">Board</A> /</div>
      <div className="page-head">
        <div className="grow">
          <h1>{workstream.title}</h1>
          <p>
            Opened by {users[workstream.created_by]?.name ?? "someone"} {timeAgo(workstream.created_at)}
            {workstream.repos.length > 0 && <> · {workstream.repos.join(", ")}</>}
          </p>
        </div>
        <div className="row">
          <select value={workstream.status} onChange={(e) => void setStatus(e.target.value)} aria-label="Stage" style={{ width: "auto" }}>
            {STATUSES.map((s) => <option key={s.value} value={s.value}>{s.label}</option>)}
          </select>
          <button className="btn" onClick={() => setEditing(true)}>Edit</button>
          <button className="btn btn-primary" onClick={() => setComposing(true)}>Add an agent</button>
        </div>
      </div>
      <ErrorNote error={action.error} />

      <div className="session-layout">
        <div className="stack" style={{ gap: 22 }}>
          {workstream.description && (
            <section className="card card-pad"><Markdown text={workstream.description} /></section>
          )}

          <section>
            <h2 style={{ marginBottom: 10 }}>Agents</h2>
            {sessions.length === 0 ? (
              <Empty title="No agents yet">Add an agent to start scoping or building this.</Empty>
            ) : (
              <div className="card">
                {roots.map((s) => (
                  <div key={s.id}>
                    <AgentRow session={s} />
                    {childrenOf(s.id).map((c) => <AgentRow key={c.id} session={c} child />)}
                  </div>
                ))}
              </div>
            )}
          </section>

          <section>
            <h2 style={{ marginBottom: 4 }}>Discussion</h2>
            <p className="small muted">For the people on the team. Agents do not read this; message an agent from its own page.</p>
            {data.comments.map((c) => (
              <div className="comment" key={c.id}>
                <Avatar user={users[c.user_id]} size={26} />
                <div className="grow">
                  <div><strong>{c.author}</strong> <span className="small faint">{timeAgo(c.created_at)}</span></div>
                  <div className="comment-body">{c.body}</div>
                </div>
              </div>
            ))}
            <div className="row" style={{ alignItems: "flex-end", marginTop: 8 }}>
              <textarea rows={2} style={{ minHeight: 0 }} placeholder="Add a comment" value={comment} onChange={(e) => setComment(e.target.value)} />
              <button className="btn" disabled={!comment.trim() || action.busy} onClick={() => void postComment()}>Comment</button>
            </div>
          </section>
        </div>

        <aside className="side">
          {workstream.links.length > 0 && (
            <div className="card card-pad">
              <h3>Links</h3>
              <div className="stack" style={{ gap: 6 }}>
                {workstream.links.map((l) => (
                  <a key={l.url} href={l.url} target="_blank" rel="noreferrer noopener" className="truncate">{l.title}</a>
                ))}
              </div>
            </div>
          )}
          <div className="card card-pad">
            <h3>Activity</h3>
            <Feed items={activity.filter((a) => a.workstream_id === id).slice(0, 20)} />
          </div>
        </aside>
      </div>

      {composing && <Composer onClose={() => setComposing(false)} workstreamId={id} workstreamRepos={workstream.repos} />}
      {editing && <EditWorkstream workstream={workstream} onClose={() => setEditing(false)} />}
    </div>
  );
}

function EditWorkstream({ workstream, onClose }: { workstream: Workstream; onClose: () => void }) {
  const [title, setTitle] = useState(workstream.title);
  const [description, setDescription] = useState(workstream.description);
  const action = useAction();
  const save = () => action.run(async () => (await api.patch(`/workstreams/${workstream.id}`, { title, description }), onClose()));
  return (
    <Modal title="Edit workstream" onClose={onClose}>
      <label className="field">Title<input type="text" value={title} onChange={(e) => setTitle(e.target.value)} /></label>
      <label className="field">
        Brief
        <textarea rows={8} value={description} onChange={(e) => setDescription(e.target.value)} />
        <span className="hint">Agents added to this workstream are given this brief.</span>
      </label>
      <ErrorNote error={action.error} />
      <div className="modal-foot">
        <button className="btn" onClick={onClose}>Cancel</button>
        <button className="btn btn-primary" disabled={!title.trim() || action.busy} onClick={() => void save()}>Save</button>
      </div>
    </Modal>
  );
}
