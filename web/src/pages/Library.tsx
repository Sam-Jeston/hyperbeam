import { useState } from "react";
import { api, type Skill, type Workflow } from "../lib/api";
import { A, Empty, ErrorNote, Modal, navigate, timeAgo, useAction, useLoad } from "../lib/ui";

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

export function LibraryPage({ tab }: { tab: string }) {
  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Library</h1>
          <p>What your team teaches its agents. Anyone on the team can add to it.</p>
        </div>
      </div>
      <div className="tabs">
        <button className={tab === "skills" ? "active" : ""} onClick={() => navigate("/library/skills")}>Skills</button>
        <button className={tab === "workflows" ? "active" : ""} onClick={() => navigate("/library/workflows")}>Workflows</button>
        <button className={tab === "notes" ? "active" : ""} onClick={() => navigate("/library/notes")}>Team notes</button>
      </div>
      {tab === "workflows" ? <Workflows /> : tab === "notes" ? <Notes /> : <Skills />}
    </div>
  );
}

function Skills() {
  const { data, error, reload } = useLoad<{ skills: Skill[] }>(() => api.get("/skills"), []);
  const [editing, setEditing] = useState<Skill | "new" | null>(null);
  return (
    <div className="section">
      <div className="spread" style={{ marginBottom: 12 }}>
        <p className="muted" style={{ margin: 0, maxWidth: "70ch" }}>
          A skill is a set of instructions an agent reads when the work calls for it: how you write migrations, how to run the test suite, your review checklist. Skills use the open SKILL.md format and work with every harness.
        </p>
        <button className="btn btn-primary" onClick={() => setEditing("new")}>New skill</button>
      </div>
      <ErrorNote error={error} />
      {data && data.skills.length === 0 && <Empty title="No skills yet">Write down something you find yourself explaining to agents more than once.</Empty>}
      {data && data.skills.length > 0 && (
        <div className="card">
          <table>
            <tbody>
              {data.skills.map((skill) => (
                <tr key={skill.id} className="clickable" onClick={() => setEditing(skill)}>
                  <td style={{ width: 220 }}><strong className="mono">{skill.name}</strong></td>
                  <td className="muted">{skill.description}</td>
                  <td className="small faint" style={{ textAlign: "right", whiteSpace: "nowrap" }}>{timeAgo(skill.updated_at)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {editing && <SkillEditor skill={editing === "new" ? null : editing} onClose={() => (setEditing(null), reload())} />}
    </div>
  );
}

function SkillEditor({ skill, onClose }: { skill: Skill | null; onClose: () => void }) {
  const [name, setName] = useState(skill?.name ?? "");
  const [description, setDescription] = useState(skill?.description ?? "");
  const [content, setContent] = useState(skill?.content ?? SKILL_TEMPLATE);
  const action = useAction();
  const save = () =>
    action.run(async () => {
      const body = { name, description, content };
      await (skill ? api.put(`/skills/${skill.id}`, body) : api.post("/skills", body));
      onClose();
    });
  const remove = () => action.run(async () => (await api.del(`/skills/${skill!.id}`), onClose()));
  return (
    <Modal title={skill ? `Edit ${skill.name}` : "New skill"} onClose={onClose} wide>
      <label className="field">Name<input type="text" value={name} onChange={(e) => setName(e.target.value)} placeholder="database-migrations" /><span className="hint">Lowercase letters, digits and dashes.</span></label>
      <label className="field">When to use it<input type="text" value={description} onChange={(e) => setDescription(e.target.value)} placeholder="Use when adding or changing a database migration" /><span className="hint">Agents decide whether to read the skill from this line, so be specific.</span></label>
      <label className="field">Instructions<textarea className="code" rows={16} value={content} onChange={(e) => setContent(e.target.value)} /></label>
      <ErrorNote error={action.error} />
      <div className="modal-foot">
        {skill && <button className="btn btn-danger" style={{ marginRight: "auto" }} disabled={action.busy} onClick={() => void remove()}>Delete</button>}
        <button className="btn" onClick={onClose}>Cancel</button>
        <button className="btn btn-primary" disabled={action.busy || !name.trim() || !content.trim()} onClick={() => void save()}>Save skill</button>
      </div>
    </Modal>
  );
}

function Workflows() {
  const { data, error, reload } = useLoad<{ workflows: Workflow[] }>(() => api.get("/workflows"), []);
  const [editing, setEditing] = useState<Workflow | "new" | null>(null);
  return (
    <div className="section">
      <div className="spread" style={{ marginBottom: 12 }}>
        <p className="muted" style={{ margin: 0, maxWidth: "70ch" }}>
          A workflow is a saved, parameterised way of starting agents: which repositories, skills and permissions, and what to ask for. Run one from “New work” on the <A href="/">board</A>.
        </p>
        <button className="btn btn-primary" onClick={() => setEditing("new")}>New workflow</button>
      </div>
      <ErrorNote error={error} />
      {data && data.workflows.length === 0 && <Empty title="No workflows yet">Capture a request your team makes often, so anyone can run it by filling in the blanks.</Empty>}
      {data && data.workflows.length > 0 && (
        <div className="card">
          <table>
            <tbody>
              {data.workflows.map((wf) => (
                <tr key={wf.id} className="clickable" onClick={() => setEditing(wf)}>
                  <td style={{ width: 220 }}><strong className="mono">{wf.name}</strong></td>
                  <td className="muted">{wf.description}</td>
                  <td>
                    <div className="row-wrap" style={{ justifyContent: "flex-end" }}>
                      {wf.spec?.role === "conductor" && <span className="chip chip-accent">Conductor</span>}
                      {wf.spec?.repos.map((r) => <span key={r} className="chip">{r}</span>)}
                      {!wf.spec && <span className="chip chip-red">Invalid</span>}
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {editing && <WorkflowEditor workflow={editing === "new" ? null : editing} onClose={() => (setEditing(null), reload())} />}
    </div>
  );
}

function WorkflowEditor({ workflow, onClose }: { workflow: Workflow | null; onClose: () => void }) {
  const [yaml, setYaml] = useState(workflow?.yaml ?? WORKFLOW_TEMPLATE);
  const action = useAction();
  const save = () =>
    action.run(async () => {
      await (workflow ? api.put(`/workflows/${workflow.id}`, { yaml }) : api.post("/workflows", { yaml }));
      onClose();
    });
  const remove = () => action.run(async () => (await api.del(`/workflows/${workflow!.id}`), onClose()));
  return (
    <Modal title={workflow ? `Edit ${workflow.name}` : "New workflow"} onClose={onClose} wide>
      <p className="muted small" style={{ margin: 0 }}>
        Written in YAML. <code>{"{{name}}"}</code> in the prompt is replaced by the input of that name. The agents get the permissions listed under <code>grants</code>, limited to what the person running the workflow holds.
      </p>
      <textarea className="code" rows={22} value={yaml} onChange={(e) => setYaml(e.target.value)} spellCheck={false} />
      <ErrorNote error={action.error} />
      <div className="modal-foot">
        {workflow && <button className="btn btn-danger" style={{ marginRight: "auto" }} disabled={action.busy} onClick={() => void remove()}>Delete</button>}
        <button className="btn" onClick={onClose}>Cancel</button>
        <button className="btn btn-primary" disabled={action.busy} onClick={() => void save()}>Save workflow</button>
      </div>
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
  return (
    <div className="section">
      <div className="spread" style={{ marginBottom: 12 }}>
        <p className="muted" style={{ margin: 0, maxWidth: "70ch" }}>
          Durable knowledge shared by every agent on the team: conventions, decisions, how things are run. Agents read these notes and add to them as they learn, so later sessions start out knowing more.
        </p>
        <button className="btn btn-primary" onClick={() => setEditing("new")}>New note</button>
      </div>
      <ErrorNote error={error} />
      {data && data.notes.length === 0 && <Empty title="No notes yet">Agents will add notes as they learn about your codebase. You can seed some yourself.</Empty>}
      {data && data.notes.length > 0 && (
        <div className="card">
          <table>
            <tbody>
              {data.notes.map((note) => (
                <tr key={note.key} className="clickable" onClick={() => setEditing(note)}>
                  <td style={{ width: 220 }}><strong className="mono">{note.key}</strong></td>
                  <td className="muted truncate" style={{ maxWidth: 420 }}>{note.content.split("\n")[0]}</td>
                  <td className="small faint" style={{ textAlign: "right", whiteSpace: "nowrap" }}>
                    {note.updated_by_session ? "by an agent · " : ""}{timeAgo(note.updated_at)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {editing && <NoteEditor note={editing === "new" ? null : editing} onClose={() => (setEditing(null), reload())} />}
    </div>
  );
}

function NoteEditor({ note, onClose }: { note: Note | null; onClose: () => void }) {
  const [key, setKey] = useState(note?.key ?? "");
  const [content, setContent] = useState(note?.content ?? "");
  const action = useAction();
  const save = () => action.run(async () => (await api.put(`/memory/${encodeURIComponent(key)}`, { content }), onClose()));
  const remove = () => action.run(async () => (await api.del(`/memory/${encodeURIComponent(note!.key)}`), onClose()));
  return (
    <Modal title={note ? note.key : "New note"} onClose={onClose} wide>
      {!note && <label className="field">Key<input type="text" value={key} onChange={(e) => setKey(e.target.value)} placeholder="api-testing" /><span className="hint">Lowercase letters, digits and dashes.</span></label>}
      {note?.updated_by_session && <p className="small muted" style={{ margin: 0 }}>Last written by <A href={`/s/${note.updated_by_session}`}>an agent session</A>.</p>}
      <textarea rows={16} value={content} onChange={(e) => setContent(e.target.value)} />
      <ErrorNote error={action.error} />
      <div className="modal-foot">
        {note && <button className="btn btn-danger" style={{ marginRight: "auto" }} disabled={action.busy} onClick={() => void remove()}>Delete</button>}
        <button className="btn" onClick={onClose}>Cancel</button>
        <button className="btn btn-primary" disabled={action.busy || !key.trim()} onClick={() => void save()}>Save note</button>
      </div>
    </Modal>
  );
}
