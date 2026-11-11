// "New work": the one place people start agents from. Deliberately usable by
// someone who has never opened a terminal.

import { useEffect, useMemo, useState } from "react";
import { api, type GrantInfo, type Harness, modelLabel, type Repo, type Skill, type Workflow } from "../lib/api";
import { useStore } from "../lib/store";
import { A, ErrorNote, Modal, navigate, useAction } from "../lib/ui";

interface Props {
  onClose: () => void;
  /** Add an agent to an existing workstream instead of opening a new one. */
  workstreamId?: string;
  workstreamRepos?: string[];
}

export function Composer({ onClose, workstreamId, workstreamRepos }: Props) {
  const me = useStore((s) => s.me)!;
  const meta = useStore((s) => s.meta)!;
  const [harnesses, setHarnesses] = useState<Harness[]>([]);
  const [repos, setRepos] = useState<Repo[]>([]);
  const [skills, setSkills] = useState<Skill[]>([]);
  const [workflows, setWorkflows] = useState<Workflow[]>([]);
  const [defaultHarness, setDefaultHarness] = useState("claude-code");
  const [loaded, setLoaded] = useState(false);

  const [mode, setMode] = useState<"prompt" | "workflow">("prompt");
  const [prompt, setPrompt] = useState("");
  const [title, setTitle] = useState("");
  const [role, setRole] = useState<"worker" | "conductor">("worker");
  const [harness, setHarness] = useState("");
  // "" means the harness profile's default.
  const [model, setModel] = useState("");
  const [pickedRepos, setPickedRepos] = useState<string[]>(workstreamRepos ?? []);
  const [pickedSkills, setPickedSkills] = useState<string[]>([]);
  const [grants, setGrants] = useState<string[]>(me.grants);
  const [track, setTrack] = useState(true);
  const [workflowId, setWorkflowId] = useState("");
  const [inputs, setInputs] = useState<Record<string, string>>({});
  const action = useAction();

  useEffect(() => {
    void Promise.all([api.get("/harnesses"), api.get("/repos"), api.get("/skills"), api.get("/workflows"), api.get("/settings")]).then(
      ([h, r, s, w, settings]) => {
        setHarnesses(h.harnesses);
        setRepos(r.repos.filter((repo: Repo) => repo.accessible));
        setSkills(s.skills);
        setWorkflows(w.workflows.filter((wf: Workflow) => wf.spec));
        setDefaultHarness(settings.default_harness);
        const preferred = h.harnesses.find((x: Harness) => x.name === settings.default_harness && x.ready) ?? h.harnesses.find((x: Harness) => x.ready && x.name !== "mock");
        setHarness(preferred?.name ?? settings.default_harness);
        setLoaded(true);
      },
      (e) => action.setError(e.message),
    );
  }, []);

  const workflow = workflows.find((w) => w.id === workflowId);
  // In workflow mode the workflow names the harness.
  const harnessName = mode === "workflow" ? workflow?.spec?.harness ?? defaultHarness : harness;
  const chosenHarness = harnesses.find((h) => h.name === harnessName);
  const heldGrants: GrantInfo[] = useMemo(() => meta.grants.filter((g) => me.grants.includes(g.id)), [meta, me]);
  const needsRead = pickedRepos.length > 0 && !grants.includes("github:read");

  const toggle = (list: string[], value: string) => (list.includes(value) ? list.filter((v) => v !== value) : [...list, value]);

  const start = () =>
    action.run(async () => {
      const place = workstreamId ? { workstream_id: workstreamId } : track ? { new_workstream: title.trim() || firstLine(mode === "workflow" ? workflow?.spec?.name ?? "" : prompt) } : {};
      let result;
      if (mode === "workflow" && workflow) {
        result = await api.post(`/workflows/${workflow.id}/run`, { inputs, title: title.trim() || undefined, model: model || undefined, ...place });
      } else {
        result = await api.post("/sessions", {
          prompt,
          title: title.trim() || undefined,
          role,
          harness,
          model: model || undefined,
          repos: pickedRepos,
          skills: pickedSkills,
          grants,
          ...place,
        });
      }
      onClose();
      navigate(`/s/${result.session.id}`);
    });

  const canStart = mode === "workflow" ? !!workflow : prompt.trim().length > 0 && !!chosenHarness?.ready && !needsRead;

  return (
    <Modal title={workstreamId ? "Add an agent" : "New work"} onClose={onClose} wide>
      {workflows.length > 0 && (
        <div className="tabs" style={{ marginBottom: 0 }}>
          <button className={mode === "prompt" ? "active" : ""} onClick={() => setMode("prompt")}>Describe it</button>
          <button className={mode === "workflow" ? "active" : ""} onClick={() => setMode("workflow")}>Run a workflow</button>
        </div>
      )}

      {mode === "prompt" ? (
        <>
          <label className="field">
            What should get done?
            <textarea
              autoFocus
              rows={5}
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="Describe the outcome you want, the way you would brief a colleague. Mention anything it must not break and how you will know it is done."
            />
          </label>

          <div className="row" style={{ alignItems: "stretch" }}>
            {(["worker", "conductor"] as const).map((r) => (
              <label key={r} className="card card-pad grow" style={{ cursor: "pointer", borderColor: role === r ? "var(--accent)" : undefined }}>
                <div className="row">
                  <input type="radio" name="role" checked={role === r} onChange={() => setRole(r)} />
                  <strong>{r === "worker" ? "One agent" : "Conductor"}</strong>
                </div>
                <div className="small muted" style={{ marginTop: 4 }}>
                  {r === "worker" ? "A single agent does the work in its own sandbox." : "Scopes the work, then starts and coordinates several agents. Best for work spanning repositories."}
                </div>
              </label>
            ))}
          </div>

          <div>
            <div className="field">Repositories</div>
            {repos.length === 0 ? (
              <p className="small muted">No repositories are available to you yet. {me.role !== "member" ? <A href="/settings/repositories">Register one</A> : "Ask an admin to register one."}</p>
            ) : (
              <div className="row-wrap" style={{ marginTop: 4 }}>
                {repos.map((repo) => (
                  <label key={repo.id} className="check chip" style={pickedRepos.includes(repo.full_name) ? { background: "var(--accent-soft)", color: "var(--accent)" } : undefined}>
                    <input type="checkbox" checked={pickedRepos.includes(repo.full_name)} onChange={() => setPickedRepos(toggle(pickedRepos, repo.full_name))} />
                    {repo.full_name}
                  </label>
                ))}
              </div>
            )}
          </div>

          <div className="row" style={{ alignItems: "flex-start" }}>
            <label className="field grow">
              Harness
              <select value={harness} onChange={(e) => (setHarness(e.target.value), setModel(""))}>
                {harnesses.map((h) => (
                  <option key={h.id} value={h.name}>
                    {h.title}{h.name === defaultHarness ? " (team default)" : ""}{h.ready ? "" : " — needs a credential"}
                  </option>
                ))}
              </select>
            </label>
            <ModelPicker harness={chosenHarness} model={model} onChange={setModel} />
          </div>

          <details>
            <summary className="muted" style={{ cursor: "pointer" }}>Skills and permissions</summary>
            <div className="stack" style={{ marginTop: 12 }}>
              {skills.length > 0 && (
                <div>
                  <div className="field">Skills</div>
                  <div className="row-wrap" style={{ marginTop: 4 }}>
                    {skills.map((skill) => (
                      <label key={skill.id} className="check chip" title={skill.description}>
                        <input type="checkbox" checked={pickedSkills.includes(skill.name)} onChange={() => setPickedSkills(toggle(pickedSkills, skill.name))} />
                        {skill.name}
                      </label>
                    ))}
                  </div>
                </div>
              )}
              <div>
                <div className="field">What this agent may do</div>
                <p className="hint">It can never do more than you can. Untick anything it does not need.</p>
                {heldGrants.length === 0 ? (
                  <p className="small muted">You hold no GitHub, Linear or Notion permissions, so the agent will work without them.</p>
                ) : (
                  <div className="stack" style={{ gap: 4, marginTop: 6 }}>
                    {heldGrants.map((g) => (
                      <label key={g.id} className="check">
                        <input type="checkbox" checked={grants.includes(g.id)} onChange={() => setGrants(toggle(grants, g.id))} />
                        <span><span style={{ textTransform: "capitalize" }}>{g.system}</span>: {g.label.toLowerCase()}</span>
                      </label>
                    ))}
                  </div>
                )}
              </div>
            </div>
          </details>
        </>
      ) : (
        <>
          <label className="field">
            Workflow
            <select value={workflowId} onChange={(e) => (setWorkflowId(e.target.value), setInputs({}), setModel(""))}>
              <option value="">Choose a workflow…</option>
              {workflows.map((w) => <option key={w.id} value={w.id}>{w.name}</option>)}
            </select>
          </label>
          {workflow?.spec && (
            <>
              {workflow.spec.description && <p className="muted">{workflow.spec.description}</p>}
              <div className="row-wrap">
                <span className="chip chip-accent">{workflow.spec.role === "conductor" ? "Conductor" : "One agent"}</span>
                {workflow.spec.repos.map((r) => <span key={r} className="chip">{r}</span>)}
                {workflow.spec.grants.map((g) => <span key={g} className="chip">{g}</span>)}
              </div>
              <ModelPicker
                harness={chosenHarness}
                model={model}
                onChange={setModel}
                fallback={workflow.spec.model ? `Workflow’s choice (${modelLabel(chosenHarness?.model_choices ?? [], workflow.spec.model)})` : undefined}
              />
              {workflow.spec.inputs.map((input) => (
                <label key={input.name} className="field">
                  {input.name}{input.required && !input.default ? "" : " (optional)"}
                  <textarea rows={2} style={{ minHeight: 0 }} value={inputs[input.name] ?? ""} placeholder={input.default ?? ""} onChange={(e) => setInputs({ ...inputs, [input.name]: e.target.value })} />
                  {input.description && <span className="hint">{input.description}</span>}
                </label>
              ))}
            </>
          )}
        </>
      )}

      {!workstreamId && (
        <div className="stack" style={{ gap: 8 }}>
          <label className="check">
            <input type="checkbox" checked={track} onChange={(e) => setTrack(e.target.checked)} />
            Put this on the board as a workstream
          </label>
          {track && (
            <input type="text" value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Workstream title (optional: defaults to the first line of the request)" />
          )}
        </div>
      )}

      {loaded && mode === "prompt" && chosenHarness && !chosenHarness.ready && (
        <div className="warn-note">
          {chosenHarness.title} has no credential yet. <A href="/settings/harnesses">Add one in Settings</A> before starting.
        </div>
      )}
      {needsRead && <div className="warn-note">Checking out repositories needs “Read repositories” (github:read), which this agent would not hold.</div>}
      <ErrorNote error={action.error} />
      <div className="modal-foot">
        <button className="btn" onClick={onClose}>Cancel</button>
        <button className="btn btn-primary" disabled={!canStart || action.busy} onClick={() => void start()}>
          {action.busy ? "Starting…" : role === "conductor" && mode === "prompt" ? "Start conductor" : "Start agent"}
        </button>
      </div>
    </Modal>
  );
}

/** Choose a model for the session, or leave it to the harness profile. */
function ModelPicker(props: { harness: Harness | undefined; model: string; onChange: (model: string) => void; fallback?: string }) {
  const choices = props.harness?.model_choices ?? [];
  const profileDefault = modelLabel(choices, props.harness?.default_model);
  const fallback = props.fallback ?? (profileDefault ? `Default (${profileDefault})` : "Harness default");
  if (choices.length === 0) {
    return (
      <label className="field grow">
        Model
        <input type="text" value={props.model} placeholder={fallback} onChange={(e) => props.onChange(e.target.value.trim())} />
        <span className="hint">This harness has not reported its models yet; it will after its first session. Leave blank for its default.</span>
      </label>
    );
  }
  const chosen = choices.find((m) => m.id === props.model);
  return (
    <label className="field grow">
      Model
      <select value={props.model} onChange={(e) => props.onChange(e.target.value)}>
        <option value="">{fallback}</option>
        {choices.map((m) => <option key={m.id} value={m.id}>{m.label || m.id}</option>)}
      </select>
      {chosen?.description && <span className="hint">{chosen.description}</span>}
    </label>
  );
}

function firstLine(text: string): string {
  const line = text.split("\n").find((l) => l.trim()) ?? "Untitled";
  return line.length > 80 ? `${line.slice(0, 79)}…` : line;
}
