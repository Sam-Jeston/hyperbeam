import { Bot, NotebookPen, Plus, Sparkles, Trash2, Workflow as WorkflowIcon } from "lucide-react";
import { type ReactNode, useState } from "react";
import { Button } from "../components/ui/button";
import { confirm, Modal } from "../components/ui/dialog";
import { Field, Input, Textarea } from "../components/ui/input";
import { LinkTabs } from "../components/ui/menu";
import { Badge, Card, EmptyState, Skeleton } from "../components/ui/surface";
import { api, type Skill, type Workflow } from "../lib/api";
import { A, ErrorNote, isSubmitKey, navigate, Page, PageHeader, Time, useAction, useDocumentTitle, useLoad } from "../lib/ui";

const WORKFLOW_TEMPLATE = `name: ship-feature
description: Build a feature across repositories and open pull requests
role: conductor            # or: worker
# harness: claude-code     # defaults to the team's
# model: sonnet            # defaults to the harness profile's
repos:
  - owner/api
  - owner/web
skills: []
grants:                    # the most these agents may do
  - github:write
  - github:open_pr
inputs:
  - name: feature
    description: What to build, and how you will know it works
prompt: |
  Build this feature: {{feature}}

  Scope it first, then delegate each repository's changes to a worker.
  Open a pull request per repository and report the links.
`;

const SKILL_TEMPLATE = `Explain when this skill applies and exactly what to do.

## Steps
1. …

## Conventions
- …
`;

const TABS = [
  { value: "skills", label: "Skills", href: "/library/skills" },
  { value: "workflows", label: "Workflows", href: "/library/workflows" },
  { value: "notes", label: "Team notes", href: "/library/notes" },
];

export function LibraryPage({ tab }: { tab: string }) {
  useDocumentTitle("Library");
  return (
    <Page>
      <PageHeader title="Library" description="What your team teaches its agents. Anyone on the team can add to it." />
      <LinkTabs className="mb-6" active={tab} tabs={TABS} onNavigate={navigate} />
      {tab === "workflows" ? <Workflows /> : tab === "notes" ? <Notes /> : <Skills />}
    </Page>
  );
}

function Intro({ children, action }: { children: ReactNode; action: ReactNode }) {
  return (
    <div className="mb-4 flex flex-wrap items-start justify-between gap-4">
      <p className="max-w-[72ch] text-[13.5px] text-muted">{children}</p>
      {action}
    </div>
  );
}

function ListSkeleton() {
  return <Card className="divide-y divide-border">{[0, 1, 2].map((i) => <div key={i} className="flex gap-4 px-4 py-3.5"><Skeleton className="h-4 w-40" /><Skeleton className="h-4 flex-1" /></div>)}</Card>;
}

/** A row in a library list: a name, what it is for, and something on the right. */
function Row(props: { name: string; detail: ReactNode; aside: ReactNode; onOpen: () => void }) {
  return (
    <button onClick={props.onOpen} className="grid w-full gap-x-6 gap-y-0.5 px-4 py-3 text-left transition-colors hover:bg-surface-2/60 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring sm:grid-cols-[200px_minmax(0,1fr)_auto] sm:items-center">
      <span className="truncate font-mono text-[12.5px] font-medium text-foreground">{props.name}</span>
      <span className="truncate text-[13px] text-muted">{props.detail}</span>
      <span className="flex items-center gap-1.5 text-[12px] text-faint sm:justify-end">{props.aside}</span>
    </button>
  );
}

function Skills() {
  const { data, error, reload } = useLoad<{ skills: Skill[] }>(() => api.get("/skills"), []);
  const [editing, setEditing] = useState<Skill | "new" | null>(null);
  const create = <Button variant="primary" onClick={() => setEditing("new")}><Plus /> New skill</Button>;
  return (
    <>
      <Intro action={create}>
        A skill is a set of instructions an agent reads when the work calls for it: how you write migrations, how to run the test suite, your review checklist. Skills use the open SKILL.md format and work with every harness.
      </Intro>
      <ErrorNote error={error} />
      {!data && !error && <ListSkeleton />}
      {data && data.skills.length === 0 && (
        <EmptyState icon={<Sparkles />} title="No skills yet" action={create}>Write down something you find yourself explaining to agents more than once.</EmptyState>
      )}
      {data && data.skills.length > 0 && (
        <Card className="divide-y divide-border overflow-hidden">
          {data.skills.map((skill) => <Row key={skill.id} name={skill.name} detail={skill.description} aside={<Time iso={skill.updated_at} />} onOpen={() => setEditing(skill)} />)}
        </Card>
      )}
      {editing && <SkillEditor skill={editing === "new" ? null : editing} onClose={() => (setEditing(null), reload())} />}
    </>
  );
}

function DeleteButton({ what, onDelete, busy }: { what: string; onDelete: () => void; busy: boolean }) {
  return (
    <Button
      variant="danger-ghost"
      className="mr-auto"
      disabled={busy}
      onClick={async () => {
        if (await confirm({ title: `Delete ${what}?`, description: "This cannot be undone.", confirmLabel: "Delete", destructive: true })) onDelete();
      }}
    >
      <Trash2 /> Delete
    </Button>
  );
}

function SkillEditor({ skill, onClose }: { skill: Skill | null; onClose: () => void }) {
  const [name, setName] = useState(skill?.name ?? "");
  const [description, setDescription] = useState(skill?.description ?? "");
  const [content, setContent] = useState(skill?.content ?? SKILL_TEMPLATE);
  const action = useAction();
  const valid = !!name.trim() && !!content.trim();
  const save = () =>
    valid &&
    action.run(async () => {
      const body = { name, description, content };
      await (skill ? api.put(`/skills/${skill.id}`, body) : api.post("/skills", body));
      onClose();
    }, { success: skill ? "Skill saved" : "Skill added" });
  const remove = () => action.run(async () => (await api.del(`/skills/${skill!.id}`), onClose()), { success: "Skill deleted" });
  return (
    <Modal
      title={skill ? skill.name : "New skill"}
      onClose={onClose}
      size="lg"
      modalOnly
      footer={
        <>
          {skill && <DeleteButton what={`the skill “${skill.name}”`} busy={action.busy} onDelete={() => void remove()} />}
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!valid} loading={action.busy} onClick={() => void save()}>Save skill</Button>
        </>
      }
    >
      <div className="grid gap-4 sm:grid-cols-[220px_minmax(0,1fr)]">
        <Field label="Name" hint="Lowercase letters, digits and dashes.">
          {(id) => <Input id={id} mono value={name} onChange={(e) => setName(e.target.value)} placeholder="database-migrations" />}
        </Field>
        <Field label="When to use it" hint="Agents decide whether to read the skill from this line, so be specific.">
          {(id) => <Input id={id} value={description} onChange={(e) => setDescription(e.target.value)} placeholder="Use when adding or changing a database migration" />}
        </Field>
      </div>
      <Field label="Instructions">
        {(id) => <Textarea id={id} mono rows={16} value={content} onChange={(e) => setContent(e.target.value)} onKeyDown={(e) => isSubmitKey(e) && void save()} />}
      </Field>
      <ErrorNote error={action.error} />
    </Modal>
  );
}

function Workflows() {
  const { data, error, reload } = useLoad<{ workflows: Workflow[] }>(() => api.get("/workflows"), []);
  const [editing, setEditing] = useState<Workflow | "new" | null>(null);
  const create = <Button variant="primary" onClick={() => setEditing("new")}><Plus /> New workflow</Button>;
  return (
    <>
      <Intro action={create}>
        A workflow is a saved, parameterised way of starting agents: which repositories, skills and permissions, and what to ask for. Run one from <span className="font-medium text-foreground">New work</span>.
      </Intro>
      <ErrorNote error={error} />
      {!data && !error && <ListSkeleton />}
      {data && data.workflows.length === 0 && (
        <EmptyState icon={<WorkflowIcon />} title="No workflows yet" action={create}>Capture a request your team makes often, so anyone can run it by filling in the blanks.</EmptyState>
      )}
      {data && data.workflows.length > 0 && (
        <Card className="divide-y divide-border overflow-hidden">
          {data.workflows.map((wf) => (
            <Row
              key={wf.id}
              name={wf.name}
              detail={wf.description}
              onOpen={() => setEditing(wf)}
              aside={
                !wf.spec ? <Badge tone="fail">Invalid</Badge> : (
                  <>
                    {wf.spec.role === "conductor" && <Badge tone="primary">Conductor</Badge>}
                    {wf.spec.repos.slice(0, 2).map((r) => <Badge key={r} mono>{r}</Badge>)}
                    {wf.spec.repos.length > 2 && <span>+{wf.spec.repos.length - 2}</span>}
                  </>
                )
              }
            />
          ))}
        </Card>
      )}
      {editing && <WorkflowEditor workflow={editing === "new" ? null : editing} onClose={() => (setEditing(null), reload())} />}
    </>
  );
}

function WorkflowEditor({ workflow, onClose }: { workflow: Workflow | null; onClose: () => void }) {
  const [yaml, setYaml] = useState(workflow?.yaml ?? WORKFLOW_TEMPLATE);
  const action = useAction();
  const save = () =>
    action.run(async () => {
      await (workflow ? api.put(`/workflows/${workflow.id}`, { yaml }) : api.post("/workflows", { yaml }));
      onClose();
    }, { success: workflow ? "Workflow saved" : "Workflow added" });
  const remove = () => action.run(async () => (await api.del(`/workflows/${workflow!.id}`), onClose()), { success: "Workflow deleted" });
  return (
    <Modal
      title={workflow ? workflow.name : "New workflow"}
      description={<>Written in YAML. <code className="font-mono text-[12px]">{"{{name}}"}</code> in the prompt is replaced by the input of that name. Agents get the permissions under <code className="font-mono text-[12px]">grants</code>, limited to what the person running it holds.</>}
      onClose={onClose}
      size="lg"
      modalOnly
      footer={
        <>
          {workflow && <DeleteButton what={`the workflow “${workflow.name}”`} busy={action.busy} onDelete={() => void remove()} />}
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" loading={action.busy} onClick={() => void save()}>Save workflow</Button>
        </>
      }
    >
      <Textarea mono rows={22} aria-label="Workflow YAML" value={yaml} onChange={(e) => setYaml(e.target.value)} onKeyDown={(e) => isSubmitKey(e) && void save()} spellCheck={false} />
      <ErrorNote error={action.error} />
    </Modal>
  );
}

interface Note {
  key: string;
  content: string;
  updated_at: string;
  updated_by_session: string | null;
}

function Notes() {
  const { data, error, reload } = useLoad<{ notes: Note[] }>(() => api.get("/memory"), []);
  const [editing, setEditing] = useState<Note | "new" | null>(null);
  const create = <Button variant="primary" onClick={() => setEditing("new")}><Plus /> New note</Button>;
  return (
    <>
      <Intro action={create}>
        Durable knowledge shared by every agent on the team: conventions, decisions, how things are run. Agents read these notes and add to them as they learn, so later sessions start out knowing more.
      </Intro>
      <ErrorNote error={error} />
      {!data && !error && <ListSkeleton />}
      {data && data.notes.length === 0 && (
        <EmptyState icon={<NotebookPen />} title="No notes yet" action={create}>Agents will add notes as they learn about your codebase. You can seed some yourself.</EmptyState>
      )}
      {data && data.notes.length > 0 && (
        <Card className="divide-y divide-border overflow-hidden">
          {data.notes.map((note) => (
            <Row
              key={note.key}
              name={note.key}
              detail={note.content.split("\n")[0]}
              onOpen={() => setEditing(note)}
              aside={<>{note.updated_by_session && <Badge><Bot /> Agent</Badge>}<Time iso={note.updated_at} /></>}
            />
          ))}
        </Card>
      )}
      {editing && <NoteEditor note={editing === "new" ? null : editing} onClose={() => (setEditing(null), reload())} />}
    </>
  );
}

function NoteEditor({ note, onClose }: { note: Note | null; onClose: () => void }) {
  const [key, setKey] = useState(note?.key ?? "");
  const [content, setContent] = useState(note?.content ?? "");
  const action = useAction();
  const save = () => key.trim() && action.run(async () => (await api.put(`/memory/${encodeURIComponent(key)}`, { content }), onClose()), { success: "Note saved" });
  const remove = () => action.run(async () => (await api.del(`/memory/${encodeURIComponent(note!.key)}`), onClose()), { success: "Note deleted" });
  return (
    <Modal
      title={note ? note.key : "New note"}
      description={note?.updated_by_session ? <>Last written by <A href={`/s/${note.updated_by_session}`} className="text-primary hover:underline">an agent session</A>.</> : undefined}
      onClose={onClose}
      size="lg"
      modalOnly
      footer={
        <>
          {note && <DeleteButton what={`the note “${note.key}”`} busy={action.busy} onDelete={() => void remove()} />}
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!key.trim()} loading={action.busy} onClick={() => void save()}>Save note</Button>
        </>
      }
    >
      {!note && (
        <Field label="Key" hint="Lowercase letters, digits and dashes.">
          {(id) => <Input id={id} mono value={key} onChange={(e) => setKey(e.target.value)} placeholder="api-testing" />}
        </Field>
      )}
      <Textarea rows={16} aria-label="Note" value={content} onChange={(e) => setContent(e.target.value)} onKeyDown={(e) => isSubmitKey(e) && void save()} />
      <ErrorNote error={action.error} />
    </Modal>
  );
}
