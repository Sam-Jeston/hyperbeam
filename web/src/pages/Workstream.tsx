import { Bot, ExternalLink, MessageSquare, Pencil, Plus } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Composer } from "../components/Composer";
import { Button } from "../components/ui/button";
import { Modal } from "../components/ui/dialog";
import { Field, Input, Kbd, Textarea } from "../components/ui/input";
import { Select } from "../components/ui/select";
import { Card, EmptyState, Panel, Skeleton } from "../components/ui/surface";
import { api, type Session, type Workstream, type WorkstreamStatus } from "../lib/api";
import { useStore } from "../lib/store";
import { Avatar, ErrorNote, Ext, isSubmitKey, Markdown, modKey, Page, PageHeader, SectionTitle, Time, useAction, useDocumentTitle, useLoad, useTick } from "../lib/ui";
import { cn } from "../lib/utils";
import { AgentRow, Feed, STAGES } from "./Board";

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
  const action = useAction();
  const workstream = live ?? data?.workstream;
  useDocumentTitle(workstream?.title);

  useEffect(() => {
    const onChange = (e: Event) => (e as CustomEvent).detail === id && reload();
    window.addEventListener("hyperbeam:workstream", onChange);
    return () => window.removeEventListener("hyperbeam:workstream", onChange);
  }, [id]);

  const sessions = useMemo(
    () => Object.values(allSessions).filter((s) => s.workstream_id === id).sort((a, b) => a.created_at.localeCompare(b.created_at)),
    [allSessions, id],
  );

  if (error) return <Page><ErrorNote title="Could not open this workstream" error={error} /></Page>;
  if (!data || !workstream) {
    return (
      <Page>
        <Skeleton className="mb-3 h-3.5 w-24" />
        <Skeleton className="mb-8 h-7 w-96 max-w-full" />
        <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_300px]"><Skeleton className="h-64 rounded-lg" /><Skeleton className="h-48 rounded-lg" /></div>
      </Page>
    );
  }
  const roots = sessions.filter((s) => !s.parent_id || !sessions.some((p) => p.id === s.parent_id));
  const childrenOf = (parent: string) => sessions.filter((s) => s.parent_id === parent);
  const stageLabel = (v: WorkstreamStatus) => STAGES.find((s) => s.status === v)?.label ?? "Archived";

  const setStatus = (status: string) =>
    action.run(() => api.patch(`/workstreams/${id}`, { status }), { success: `Moved to ${stageLabel(status as WorkstreamStatus)}`, toastError: true });

  return (
    <Page>
      <PageHeader
        breadcrumbs={[{ label: "Board", href: "/" }]}
        title={workstream.title}
        meta={
          <>
            <span className="flex items-center gap-1.5"><Avatar user={users[workstream.created_by]} size={18} className="ring-0" /> Opened by {users[workstream.created_by]?.name ?? "someone"}</span>
            <span className="text-faint">·</span>
            <Time iso={workstream.created_at} />
            {workstream.repos.map((r) => <span key={r} className="rounded-md bg-surface-2 px-1.5 py-0.5 font-mono text-[11.5px]">{r}</span>)}
          </>
        }
        actions={
          <>
            <Select
              aria-label="Stage"
              className="w-36"
              value={workstream.status}
              onValueChange={(v) => void setStatus(v)}
              options={[
                ...STAGES.map((s) => ({ value: s.status, label: <span className="flex items-center gap-2"><span className={cn("size-2 rounded-full", s.dot)} />{s.label}</span> })),
                { value: "archived", label: <span className="flex items-center gap-2"><span className="size-2 rounded-full bg-faint" />Archived</span> },
              ]}
            />
            <Button onClick={() => setEditing(true)}><Pencil /> Edit</Button>
            <Button variant="primary" onClick={() => setComposing(true)}><Plus /> Add an agent</Button>
          </>
        }
      />

      <div className="grid items-start gap-6 lg:grid-cols-[minmax(0,1fr)_300px]">
        <div className="grid min-w-0 gap-8">
          {workstream.description ? (
            <Card className="px-5 py-4">
              <div className="mb-2 text-[11.5px] font-semibold uppercase tracking-[0.07em] text-faint">Brief</div>
              <Markdown text={workstream.description} className="text-[14px]" />
            </Card>
          ) : (
            <button onClick={() => setEditing(true)} className="rounded-lg border border-dashed border-border-strong px-5 py-4 text-left text-[13.5px] text-muted transition-colors hover:border-faint hover:text-foreground">
              <span className="font-medium text-foreground">Add a brief.</span> Agents added to this workstream are given it, so they start with the context you would give a colleague.
            </button>
          )}

          <section>
            <SectionTitle>Agents</SectionTitle>
            {sessions.length === 0 ? (
              <EmptyState compact icon={<Bot />} title="No agents yet" action={<Button variant="primary" size="sm" onClick={() => setComposing(true)}><Plus /> Add an agent</Button>}>
                Add an agent to start scoping or building this.
              </EmptyState>
            ) : (
              <Card className="divide-y divide-border overflow-hidden">
                {roots.map((s) => (
                  <div key={s.id} className="divide-y divide-border">
                    <AgentRow session={s} />
                    {childrenOf(s.id).map((c) => <AgentRow key={c.id} session={c} child />)}
                  </div>
                ))}
              </Card>
            )}
          </section>

          <Discussion id={id} comments={data.comments} onPosted={reload} />
        </div>

        <aside className="grid gap-4 lg:sticky lg:top-6">
          {workstream.links.length > 0 && (
            <Panel title="Links">
              <ul className="grid gap-1.5 text-[13px]">
                {workstream.links.map((l) => (
                  <li key={l.url} className="min-w-0"><Ext href={l.url} className="flex items-start gap-1.5"><ExternalLink className="mt-0.5 size-3.5 shrink-0" /><span className="[overflow-wrap:anywhere]">{l.title}</span></Ext></li>
                ))}
              </ul>
            </Panel>
          )}
          <Panel title="Activity">
            <Feed items={activity.filter((a) => a.workstream_id === id).slice(0, 20)} />
          </Panel>
        </aside>
      </div>

      {composing && <Composer onClose={() => setComposing(false)} workstreamId={id} workstreamRepos={workstream.repos} />}
      {editing && <EditWorkstream workstream={workstream} onClose={() => setEditing(false)} />}
    </Page>
  );
}

function Discussion({ id, comments, onPosted }: { id: string; comments: Comment[]; onPosted: () => void }) {
  const users = useStore((s) => s.users);
  const me = useStore((s) => s.me)!;
  const [comment, setComment] = useState("");
  const action = useAction();
  const post = () => {
    if (!comment.trim() || action.busy) return;
    void action.run(async () => {
      await api.post(`/workstreams/${id}/comments`, { body: comment });
      setComment("");
      onPosted();
    });
  };
  return (
    <section>
      <SectionTitle description="For the people on the team. Agents do not read this; message an agent from its own page.">Discussion</SectionTitle>
      <div className="grid gap-4">
        {comments.length === 0 && (
          <p className="flex items-center gap-2 text-[13px] text-faint"><MessageSquare className="size-4" /> No comments yet.</p>
        )}
        {comments.map((c) => (
          <div key={c.id} className="flex gap-3">
            <Avatar user={users[c.user_id]} size={28} className="ring-0" />
            <div className="min-w-0 flex-1">
              <div className="flex items-baseline gap-2 text-[13px]">
                <span className="font-semibold">{c.author}</span>
                <Time iso={c.created_at} className="text-[12px] text-faint" />
              </div>
              <div className="mt-0.5 text-[14px] leading-relaxed whitespace-pre-wrap [overflow-wrap:anywhere]">{c.body}</div>
            </div>
          </div>
        ))}
        <div className="flex gap-3">
          <Avatar user={me} size={28} className="ring-0" />
          <div className="grid min-w-0 flex-1 gap-2">
            <Textarea
              rows={2}
              className="min-h-16"
              placeholder="Add a comment"
              aria-label="Comment"
              value={comment}
              onChange={(e) => setComment(e.target.value)}
              onKeyDown={(e) => isSubmitKey(e) && (e.preventDefault(), post())}
            />
            <ErrorNote error={action.error} />
            {comment.trim() && (
              <div className="flex items-center justify-end gap-2">
                <span className="text-[12px] text-faint"><Kbd>{modKey}</Kbd> <Kbd>Enter</Kbd></span>
                <Button variant="primary" size="sm" loading={action.busy} onClick={post}>Comment</Button>
              </div>
            )}
          </div>
        </div>
      </div>
    </section>
  );
}

function EditWorkstream({ workstream, onClose }: { workstream: Workstream; onClose: () => void }) {
  const [title, setTitle] = useState(workstream.title);
  const [description, setDescription] = useState(workstream.description);
  const action = useAction();
  const save = () => action.run(async () => (await api.patch(`/workstreams/${workstream.id}`, { title, description }), onClose()), { success: "Workstream saved" });
  return (
    <Modal
      title="Edit workstream"
      onClose={onClose}
      size="lg"
      modalOnly
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!title.trim()} loading={action.busy} onClick={() => void save()}>Save</Button>
        </>
      }
    >
      <Field label="Title">{(id) => <Input id={id} value={title} onChange={(e) => setTitle(e.target.value)} />}</Field>
      <Field label="Brief" hint="Agents added to this workstream are given this brief. Markdown works.">
        {(id) => <Textarea id={id} rows={10} value={description} onChange={(e) => setDescription(e.target.value)} onKeyDown={(e) => isSubmitKey(e) && title.trim() && void save()} />}
      </Field>
      <ErrorNote error={action.error} />
    </Modal>
  );
}
