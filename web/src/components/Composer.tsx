// "New work": the one place people start agents from. Deliberately usable by
// someone who has never opened a terminal.

import * as Collapsible from "@radix-ui/react-collapsible";
import { Bot, ChevronRight, Network } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api, type GrantInfo, type Harness, modelLabel, type Repo, type Skill, type Workflow } from "../lib/api";
import { useStore } from "../lib/store";
import { A, ErrorNote, systemName, isSubmitKey, modKey, navigate, useAction } from "../lib/ui";
import { Button } from "./ui/button";
import { CheckRow, RadioCards, SwitchRow, ToggleChip } from "./ui/controls";
import { Modal } from "./ui/dialog";
import { Field, Input, Kbd, Textarea } from "./ui/input";
import { Tabs } from "./ui/menu";
import { Select } from "./ui/select";
import { Badge, Notice, Skeleton } from "./ui/surface";

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
  const extrasSummary = [
    pickedSkills.length > 0 && `${pickedSkills.length} skill${pickedSkills.length > 1 ? "s" : ""}`,
    grants.length === heldGrants.length ? "all your permissions" : `${grants.length} of ${heldGrants.length} permissions`,
  ].filter(Boolean).join(" · ");

  const canStart = mode === "workflow" ? !!workflow : prompt.trim().length > 0 && !!chosenHarness?.ready && !needsRead;

  const start = () => {
    if (!canStart || action.busy) return;
    void action.run(async () => {
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
  };

  return (
    <Modal
      title={workstreamId ? "Add an agent" : "New work"}
      description={workstreamId ? "It is given the workstream’s brief and repositories." : "Describe the outcome. An agent works on it in its own sandbox."}
      onClose={onClose}
      size="lg"
      modalOnly={prompt.trim().length > 0}
      footer={
        <>
          <span className="mr-auto hidden items-center gap-1 text-[12px] text-faint sm:flex"><Kbd>{modKey}</Kbd><Kbd>Enter</Kbd> to start</span>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!canStart} loading={action.busy} onClick={start}>
            {action.busy ? "Starting" : role === "conductor" && mode === "prompt" ? "Start conductor" : "Start agent"}
          </Button>
        </>
      }
    >
      <div className="contents" onKeyDown={(e) => isSubmitKey(e) && (e.preventDefault(), start())}>
        {workflows.length > 0 && (
          <Tabs
            className="-mt-2"
            value={mode}
            onValueChange={setMode}
            tabs={[{ value: "prompt", label: "Describe it" }, { value: "workflow", label: "Run a workflow" }]}
          />
        )}

        {!loaded && !action.error ? (
          <div className="grid gap-3"><Skeleton className="h-28" /><Skeleton className="h-16" /><Skeleton className="h-9" /></div>
        ) : mode === "prompt" ? (
          <>
            <Field label="What should get done?">
              {(id) => (
                <Textarea
                  id={id}
                  autoFocus
                  rows={5}
                  className="text-[14px]"
                  value={prompt}
                  onChange={(e) => setPrompt(e.target.value)}
                  placeholder="Describe the outcome you want, the way you would brief a colleague. Mention anything it must not break and how you will know it is done."
                />
              )}
            </Field>

            <RadioCards
              aria-label="Who does the work"
              value={role}
              onValueChange={setRole}
              options={[
                { value: "worker", icon: <Bot />, title: "One agent", description: "A single agent does the work in its own sandbox." },
                { value: "conductor", icon: <Network />, title: "Conductor", description: "Scopes the work, then starts and coordinates several agents. Best for work spanning repositories." },
              ]}
            />

            <div className="grid gap-1.5">
              <div className="text-[13px] font-medium">Repositories</div>
              {repos.length === 0 ? (
                <p className="text-[13px] text-muted">
                  No repositories are available to you yet.{" "}
                  {me.role !== "member" ? <A href="/settings/repositories" className="text-primary hover:underline">Register one</A> : "Ask an admin to register one."}
                </p>
              ) : (
                <div className="flex flex-wrap gap-1.5">
                  {repos.map((repo) => (
                    <ToggleChip key={repo.id} mono pressed={pickedRepos.includes(repo.full_name)} onPressedChange={() => setPickedRepos(toggle(pickedRepos, repo.full_name))}>
                      {repo.full_name}
                    </ToggleChip>
                  ))}
                </div>
              )}
            </div>

            <div className="grid gap-4 sm:grid-cols-2">
              <Field label="Harness">
                {(id) => (
                  <Select
                    id={id}
                    value={harness}
                    onValueChange={(v) => (setHarness(v), setModel(""))}
                    options={harnesses.map((h) => ({
                      value: h.name,
                      label: h.title,
                      description: [h.name === defaultHarness && "Team default", !h.ready && "Needs a credential"].filter(Boolean).join(" · ") || undefined,
                    }))}
                  />
                )}
              </Field>
              <ModelPicker harness={chosenHarness} model={model} onChange={setModel} />
            </div>

            <Collapsible.Root className="group rounded-lg border border-border">
              <Collapsible.Trigger className="flex w-full items-center gap-2 rounded-lg px-3 py-2.5 text-left text-[13px] hover:bg-surface-2/50">
                <ChevronRight className="size-4 text-faint transition-transform group-data-[state=open]:rotate-90" />
                <span className="font-medium">Skills and permissions</span>
                <span className="ml-auto truncate text-[12.5px] text-muted">{extrasSummary}</span>
              </Collapsible.Trigger>
              <Collapsible.Content className="grid gap-5 border-t border-border px-3.5 py-3.5">
                {skills.length > 0 && (
                  <div className="grid gap-1.5">
                    <div className="text-[13px] font-medium">Skills</div>
                    <div className="flex flex-wrap gap-1.5">
                      {skills.map((skill) => (
                        <ToggleChip key={skill.id} mono title={skill.description} pressed={pickedSkills.includes(skill.name)} onPressedChange={() => setPickedSkills(toggle(pickedSkills, skill.name))}>
                          {skill.name}
                        </ToggleChip>
                      ))}
                    </div>
                  </div>
                )}
                <div className="grid gap-2">
                  <div>
                    <div className="text-[13px] font-medium">What this agent may do</div>
                    <p className="text-[12.5px] text-muted">It can never do more than you can. Untick anything it does not need.</p>
                  </div>
                  {heldGrants.length === 0 ? (
                    <p className="text-[13px] text-muted">You hold no GitHub, Linear or Notion permissions, so the agent will work without them.</p>
                  ) : (
                    <div className="grid gap-2 sm:grid-cols-2">
                      {heldGrants.map((g) => (
                        <CheckRow key={g.id} checked={grants.includes(g.id)} onCheckedChange={() => setGrants(toggle(grants, g.id))} label={<span>{systemName(g.system)}: {g.label.toLowerCase()}</span>} />
                      ))}
                    </div>
                  )}
                </div>
              </Collapsible.Content>
            </Collapsible.Root>
          </>
        ) : (
          <>
            <Field label="Workflow">
              {(id) => (
                <Select
                  id={id}
                  value={workflowId}
                  placeholder="Choose a workflow"
                  onValueChange={(v) => (setWorkflowId(v), setInputs({}), setModel(""))}
                  options={workflows.map((w) => ({ value: w.id, label: w.name, description: w.description || undefined }))}
                />
              )}
            </Field>
            {workflow?.spec && (
              <>
                {workflow.spec.description && <p className="text-[13.5px] text-muted">{workflow.spec.description}</p>}
                <div className="flex flex-wrap gap-1.5">
                  <Badge tone="primary">{workflow.spec.role === "conductor" ? "Conductor" : "One agent"}</Badge>
                  {workflow.spec.repos.map((r) => <Badge key={r} mono>{r}</Badge>)}
                  {workflow.spec.grants.map((g) => <Badge key={g} tone="outline" mono>{g}</Badge>)}
                </div>
                {workflow.spec.inputs.map((input) => (
                  <Field key={input.name} label={input.name} optional={!(input.required && !input.default)} hint={input.description}>
                    {(id) => <Textarea id={id} rows={2} className="min-h-0" value={inputs[input.name] ?? ""} placeholder={input.default ?? ""} onChange={(e) => setInputs({ ...inputs, [input.name]: e.target.value })} />}
                  </Field>
                ))}
                <ModelPicker
                  harness={chosenHarness}
                  model={model}
                  onChange={setModel}
                  fallback={workflow.spec.model ? `Workflow’s choice (${modelLabel(chosenHarness?.model_choices ?? [], workflow.spec.model)})` : undefined}
                />
              </>
            )}
          </>
        )}

        {!workstreamId && loaded && (
          <div className="grid gap-2.5 rounded-lg bg-surface-2/50 px-3.5 py-3">
            <SwitchRow checked={track} onCheckedChange={setTrack} label="Put this on the board" description="Creates a workstream so the team can follow and discuss it." />
            {track && (
              <Input value={title} onChange={(e) => setTitle(e.target.value)} aria-label="Workstream title" placeholder="Workstream title (defaults to the first line of the request)" />
            )}
          </div>
        )}

        {loaded && mode === "prompt" && chosenHarness && !chosenHarness.ready && (
          <Notice tone="warning" title={`${chosenHarness.title} has no credential yet`}>
            <A href="/settings/harnesses" className="text-primary hover:underline">Add one in Settings</A> before starting, or pick another harness.
          </Notice>
        )}
        {needsRead && <Notice tone="warning">Checking out repositories needs “Read repositories” (github:read), which this agent would not hold.</Notice>}
        <ErrorNote error={action.error} />
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
      <Field label="Model" hint="This harness has not reported its models yet; it will after its first session. Leave blank for its default.">
        {(id) => <Input id={id} value={props.model} placeholder={fallback} onChange={(e) => props.onChange(e.target.value.trim())} />}
      </Field>
    );
  }
  const chosen = choices.find((m) => m.id === props.model);
  return (
    <Field label="Model" hint={chosen?.description}>
      {(id) => (
        <Select
          id={id}
          value={props.model}
          onValueChange={props.onChange}
          options={[{ value: "", label: fallback }, ...choices.map((m) => ({ value: m.id, label: m.label || m.id }))]}
        />
      )}
    </Field>
  );
}

function firstLine(text: string): string {
  const line = text.split("\n").find((l) => l.trim()) ?? "Untitled";
  return line.length > 80 ? `${line.slice(0, 79)}…` : line;
}
