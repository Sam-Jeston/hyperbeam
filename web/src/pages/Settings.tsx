import { Cpu, FolderGit2, KeyRound, Lock, Pencil, Plus, RefreshCw, ScrollText, Trash2, Zap } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { Button } from "../components/ui/button";
import { Switch } from "../components/ui/controls";
import { confirm, Modal } from "../components/ui/dialog";
import { Field, Input, Textarea } from "../components/ui/input";
import { LinkTabs } from "../components/ui/menu";
import { Select } from "../components/ui/select";
import { Badge, Card, EmptyState, Notice, Skeleton } from "../components/ui/surface";
import { api, type CodeHost, type CredentialSlot, type Harness, modelLabel, type Repo } from "../lib/api";
import { refreshMe, useStore } from "../lib/store";
import { Connections } from "./Connections";
import { A, CopyField, ErrorNote, navigate, Page, PageHeader, systemName, Time, useAction, useDocumentTitle, useLoad } from "../lib/ui";

const TABS: { value: string; label: string; adminOnly: boolean }[] = [
  { value: "integrations", label: "Integrations", adminOnly: false },
  { value: "repositories", label: "Repositories", adminOnly: false },
  { value: "harnesses", label: "Harnesses", adminOnly: false },
  { value: "general", label: "General", adminOnly: true },
  { value: "audit", label: "Audit log", adminOnly: true },
  { value: "account", label: "My account", adminOnly: false },
];

export function SettingsPage({ tab }: { tab: string }) {
  const me = useStore((s) => s.me)!;
  const isAdmin = me.role !== "member";
  const tabs = TABS.filter((t) => isAdmin || !t.adminOnly);
  const active = tabs.some((t) => t.value === tab) ? tab : "integrations";
  useDocumentTitle(tabs.find((t) => t.value === active)?.label ?? "Settings");
  return (
    <Page>
      <PageHeader title="Settings" description="Connect the systems your agents work with and choose how they run." />
      <LinkTabs className="mb-6" active={active} onNavigate={navigate} tabs={tabs.map((t) => ({ ...t, href: `/settings/${t.value}` }))} />
      {active === "repositories" ? <Repositories isAdmin={isAdmin} /> :
       active === "harnesses" ? <Harnesses isAdmin={isAdmin} /> :
       active === "general" ? <General /> :
       active === "audit" ? <Audit /> :
       active === "account" ? <Account /> :
       <Integrations isAdmin={isAdmin} />}
    </Page>
  );
}

function Lead({ children, action }: { children: ReactNode; action?: ReactNode }) {
  return (
    <div className="mb-5 flex flex-wrap items-start justify-between gap-4">
      <p className="max-w-[72ch] text-[13.5px] text-muted">{children}</p>
      {action}
    </div>
  );
}

function CardsSkeleton({ n = 3 }: { n?: number }) {
  return <div className="grid gap-3">{Array.from({ length: n }, (_, i) => <Skeleton key={i} className="h-24 rounded-lg" />)}</div>;
}

// -------------------------------------------------------------- integrations

interface Integration {
  kind: CodeHost;
  connected: boolean;
  /** GitHub: how it authenticates. GitLab: where the instance lives. */
  config: { mode?: string; app_id?: string; url?: string };
  updated_at?: string;
}

const GITLAB_COM = "https://gitlab.com";

const INTEGRATION_COPY: Record<CodeHost, { title: string; blurb: string; tile: string }> = {
  github: { title: "GitHub", blurb: "Lets agents clone and push to registered repositories, and work with issues and pull requests.", tile: "G" },
  gitlab: { title: "GitLab", blurb: "Lets agents clone and push to registered projects, and work with issues and merge requests. gitlab.com or your own instance.", tile: "L" },
};

/** How a connected integration is set up, in a few words. */
function integrationDetail({ kind, config }: Integration): string {
  if (kind === "github") return config.mode === "app" ? `GitHub App ${config.app_id}` : "Access token";
  return (config.url ?? GITLAB_COM).replace(/^https?:\/\//, "");
}

function Integrations({ isAdmin }: { isAdmin: boolean }) {
  const { data, error, reload } = useLoad<{ integrations: Integration[] }>(() => api.get("/integrations"), []);
  const [editing, setEditing] = useState<Integration | null>(null);
  const [results, setResults] = useState<Record<string, { ok: boolean; detail: string }>>({});
  const [testing, setTesting] = useState<string | null>(null);
  const test = async (kind: string) => {
    setTesting(kind);
    try {
      const result = await api.post(`/integrations/${kind}/test`);
      setResults((r) => ({ ...r, [kind]: result }));
    } catch (e) {
      setResults((r) => ({ ...r, [kind]: { ok: false, detail: e instanceof Error ? e.message : String(e) } }));
    } finally {
      setTesting(null);
    }
  };
  return (
    <>
      <Lead>
        These credentials stay in Hyperbeam’s control plane. Agents never see them: every request an agent makes goes through Hyperbeam, which checks it against the permissions of the person who started the agent.
      </Lead>
      <ErrorNote error={error} />
      {!data && !error && <CardsSkeleton />}
      <div className="grid gap-3">
        {data?.integrations.map((integration) => {
          const copy = INTEGRATION_COPY[integration.kind];
          const result = results[integration.kind];
          return (
            <Card key={integration.kind} className="grid gap-3 p-4">
              <div className="flex flex-wrap items-start gap-4">
                <div className="grid size-10 shrink-0 place-items-center rounded-lg bg-surface-2 font-display text-[17px] font-bold text-foreground">{copy.tile}</div>
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <h2 className="font-display text-[15.5px] font-semibold">{copy.title}</h2>
                    {integration.connected ? <Badge tone="run"><span className="size-1.5 rounded-full bg-run" />Connected</Badge> : <Badge>Not connected</Badge>}
                    {integration.connected && <Badge tone="outline">{integrationDetail(integration)}</Badge>}
                  </div>
                  <p className="mt-0.5 text-[13px] text-muted">{copy.blurb}</p>
                </div>
                {isAdmin && (
                  <div className="flex gap-2">
                    {integration.connected && <Button loading={testing === integration.kind} onClick={() => void test(integration.kind)}>{testing !== integration.kind && <Zap />} Test</Button>}
                    <Button variant={integration.connected ? "secondary" : "primary"} onClick={() => setEditing(integration)}>{integration.connected ? "Change" : "Connect"}</Button>
                  </div>
                )}
              </div>
              {result && <Notice tone={result.ok ? "success" : "error"}>{result.detail}</Notice>}
            </Card>
          );
        })}
      </div>
      {editing && <IntegrationDialog integration={editing} onClose={() => (setEditing(null), reload())} />}
      <Connections isAdmin={isAdmin} />
    </>
  );
}

function IntegrationDialog({ integration, onClose }: { integration: Integration; onClose: () => void }) {
  const [mode, setMode] = useState(integration.config.mode ?? "token");
  const [appId, setAppId] = useState(integration.config.app_id ?? "");
  const [url, setUrl] = useState(integration.config.url ?? GITLAB_COM);
  const [secret, setSecret] = useState("");
  const action = useAction();
  const { kind } = integration;
  const title = INTEGRATION_COPY[kind].title;
  const save = () =>
    action.run(async () => {
      await api.put(`/integrations/${kind}`, { config: kind === "github" ? { mode, app_id: appId } : { url }, secret: secret || undefined });
      onClose();
    }, { success: `${title} ${integration.connected ? "updated" : "connected"}` });
  const disconnect = async () => {
    const ok = await confirm({ title: `Disconnect ${title}?`, description: `Agents lose access to ${title} straight away, including ones that are running. The stored credential is deleted.`, confirmLabel: "Disconnect", destructive: true });
    if (ok) void action.run(async () => (await api.del(`/integrations/${kind}`), onClose()), { success: `${title} disconnected` });
  };
  const isApp = kind === "github" && mode === "app";
  return (
    <Modal
      title={`${integration.connected ? "Change" : "Connect"} ${title}`}
      onClose={onClose}
      modalOnly
      footer={
        <>
          {integration.connected && <Button variant="danger-ghost" className="mr-auto" disabled={action.busy} onClick={() => void disconnect()}>Disconnect</Button>}
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!integration.connected && !secret.trim()} loading={action.busy} onClick={() => void save()}>Save</Button>
        </>
      }
    >
      {kind === "github" && (
        <>
          <Field label="How Hyperbeam authenticates">
            {(id) => (
              <Select
                id={id}
                value={mode}
                onValueChange={setMode}
                options={[
                  { value: "token", label: "Access token", description: "Simplest. Good for one person or a small team." },
                  { value: "app", label: "GitHub App", description: "Recommended for organisations." },
                ]}
              />
            )}
          </Field>
          <p className="text-[13px] text-muted">
            {isApp
              ? "Create a GitHub App with repository permissions Contents, Pull requests and Issues set to read & write, install it on the repositories you will register, then paste its App ID and a generated private key. Hyperbeam mints a short-lived token per repository for each request."
              : "A fine-grained personal access token limited to the repositories you will register, with Contents, Pull requests and Issues set to read & write. Everything agents do on GitHub will appear as this token’s user."}
          </p>
          {isApp && <Field label="App ID">{(id) => <Input id={id} mono value={appId} onChange={(e) => setAppId(e.target.value)} />}</Field>}
        </>
      )}
      {kind === "gitlab" && (
        <>
          <Field label="GitLab address" hint="Change this only if your team runs its own GitLab.">
            {(id) => <Input id={id} mono value={url} onChange={(e) => setUrl(e.target.value)} placeholder={GITLAB_COM} />}
          </Field>
          <p className="text-[13px] text-muted">
            A project, group or personal access token with the <span className="font-mono text-[12.5px]">api</span> and <span className="font-mono text-[12.5px]">write_repository</span> scopes and at least the Developer role (Maintainer if agents should merge or push to protected branches). Prefer a project or group token limited to what you will register: everything agents do on GitLab will appear as this token’s user.
          </p>
        </>
      )}
      <Field
        label={isApp ? "Private key (PEM)" : "Token"}
        hint={integration.connected ? "Leave blank to keep the current one. Stored encrypted and never shown again." : "Stored encrypted and never shown again."}
      >
        {(id) =>
          isApp ? (
            <Textarea id={id} mono rows={6} value={secret} onChange={(e) => setSecret(e.target.value)} placeholder="-----BEGIN RSA PRIVATE KEY-----" />
          ) : (
            <Input id={id} type="password" mono autoComplete="off" value={secret} onChange={(e) => setSecret(e.target.value)} placeholder={integration.connected ? "••••••••••••" : undefined} />
          )
        }
      </Field>
      <ErrorNote error={action.error} />
    </Modal>
  );
}

// -------------------------------------------------------------- repositories

function Repositories({ isAdmin }: { isAdmin: boolean }) {
  const { data, error, reload } = useLoad<{ repos: Repo[] }>(() => api.get("/repos"), []);
  const integrations = useLoad<{ integrations: Integration[] }>(() => api.get("/integrations"), []);
  const [name, setName] = useState("");
  const [picked, setPicked] = useState<CodeHost | "">("");
  const action = useAction();
  // Offer a choice of host only when there is one to make.
  const connected = (integrations.data?.integrations ?? []).filter((i) => i.connected).map((i) => i.kind);
  const host = picked || connected[0] || "github";
  const add = () => name.trim() && action.run(async () => (await api.post("/repos", { full_name: name.trim(), host }), setName(""), reload()), { success: `Registered ${name.trim()}` });
  const remove = async (repo: Repo) => {
    const ok = await confirm({
      title: `Remove ${repo.full_name}?`,
      description: "New agents can no longer use it. Agents already running keep the repositories they started with until they stop. The repository itself is untouched.",
      confirmLabel: "Remove",
      destructive: true,
    });
    if (ok) void action.run(async () => (await api.del(`/repos/${repo.id}`), reload()), { success: `Removed ${repo.full_name}`, toastError: true });
  };
  return (
    <>
      <Lead>
        Agents can only reach repositories registered here, and only the ones their owner has access to (see <A href="/team" className="text-primary hover:underline">People &amp; permissions</A>).
      </Lead>
      {isAdmin && (
        <form className="mb-5 flex max-w-[640px] gap-2" onSubmit={(e) => (e.preventDefault(), void add())}>
          {connected.length > 1 && (
            <Select className="w-[130px] shrink-0" aria-label="Code host" value={host} onValueChange={(v) => setPicked(v as CodeHost)} options={connected.map((kind) => ({ value: kind, label: INTEGRATION_COPY[kind].title }))} />
          )}
          <Input mono aria-label="Repository" placeholder={host === "gitlab" ? "group/project" : "owner/name"} value={name} onChange={(e) => setName(e.target.value)} />
          <Button type="submit" variant="primary" disabled={!name.trim()} loading={action.busy}>{action.busy ? "Checking" : <><Plus /> Register</>}</Button>
        </form>
      )}
      <div className="grid gap-3">
        <ErrorNote error={error ?? action.error} />
        {!data && !error && <CardsSkeleton n={2} />}
        {data && data.repos.length === 0 && (
          <EmptyState icon={<FolderGit2 />} title="No repositories yet">
            {isAdmin ? <>Connect GitHub or GitLab under <A href="/settings/integrations" className="text-primary hover:underline">Integrations</A>, then register a repository as owner/name.</> : "Ask an admin to register a repository."}
          </EmptyState>
        )}
        {data && data.repos.length > 0 && (
          <Card className="divide-y divide-border">
            {data.repos.map((repo) => (
              <div key={repo.id} className="flex flex-wrap items-center gap-x-4 gap-y-1 px-4 py-3">
                <FolderGit2 className="size-4 shrink-0 text-faint" />
                <div className="min-w-0 flex-1">
                  <div className="font-mono text-[13px] font-medium">{repo.full_name}</div>
                  {repo.description && <div className="truncate text-[12.5px] text-muted">{repo.description}</div>}
                </div>
                {(connected.length > 1 || repo.host !== "github") && <Badge tone="outline">{systemName(repo.host)}</Badge>}
                <Badge tone="outline" mono>{repo.default_branch}</Badge>
                {repo.accessible ? <Badge tone="run">You have access</Badge> : <Badge><Lock /> No access</Badge>}
                {isAdmin && <Button variant="ghost" size="icon-sm" aria-label={`Remove ${repo.full_name}`} onClick={() => void remove(repo)}><Trash2 /></Button>}
              </div>
            ))}
          </Card>
        )}
      </div>
    </>
  );
}

// ----------------------------------------------------------------- harnesses

function Harnesses({ isAdmin }: { isAdmin: boolean }) {
  const { data, error, reload } = useLoad<{ harnesses: Harness[] }>(() => api.get("/harnesses"), []);
  const [credential, setCredential] = useState<{ harness: Harness; scope: "team" | "me" } | null>(null);
  const [editing, setEditing] = useState<Harness | "new" | null>(null);
  const refresh = useAction();
  // Model discovery runs in a short-lived sandbox; watch until it reports.
  const refreshing = data?.harnesses.some((h) => h.models_refreshing) ?? false;
  useEffect(() => {
    if (!refreshing) return;
    const timer = setInterval(reload, 2000);
    return () => clearInterval(timer);
  }, [refreshing]);
  const refreshModels = (h: Harness) => refresh.run(async () => (await api.post(`/harnesses/${h.id}/models/refresh`, {}), reload()), { toastError: true });
  return (
    <>
      <Lead action={isAdmin && <Button onClick={() => setEditing("new")}><Plus /> Add a harness</Button>}>
        A harness is the coding agent that runs inside each sandbox. Hyperbeam drives any agent that speaks the Agent Client Protocol. Give each one a credential: a team API key, or your own.
      </Lead>
      <ErrorNote error={error} />
      {!data && !error && <CardsSkeleton />}
      <div className="grid gap-3">
        {data?.harnesses.map((h) => {
          const models = h.models.length > 0 ? h.models : h.discovered_models;
          return (
            <Card key={h.id} className="overflow-hidden">
              <div className="flex flex-wrap items-start gap-4 p-4">
                <div className="grid size-10 shrink-0 place-items-center rounded-lg bg-accent text-accent-foreground"><Cpu className="size-5" /></div>
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <h2 className="font-display text-[15.5px] font-semibold">{h.title}</h2>
                    {h.ready ? <Badge tone="run"><span className="size-1.5 rounded-full bg-run" />Ready</Badge> : <Badge tone="wait">Needs a credential</Badge>}
                  </div>
                  {h.description && <p className="mt-0.5 text-[13px] text-muted">{h.description}</p>}
                  <div className="mt-1.5 truncate font-mono text-[11.5px] text-faint">{h.image} · {h.command.join(" ")}</div>
                </div>
                {isAdmin && <Button size="sm" onClick={() => setEditing(h)}><Pencil /> Edit</Button>}
              </div>

              <div className="grid gap-3 border-t border-border bg-surface-2/30 px-4 py-3 text-[13px]">
                <div className="flex flex-wrap items-center gap-x-2 gap-y-1.5">
                  <span className="w-28 shrink-0 text-muted">Models</span>
                  <Badge tone="primary">Default: {modelLabel(h.model_choices, h.default_model) ?? "the harness’s own"}</Badge>
                  {models.slice(0, 6).map((m) => <Badge key={m.id} tone="outline">{m.label || m.id}</Badge>)}
                  {models.length > 6 && <span className="text-[12px] text-faint">+{models.length - 6} more</span>}
                  {h.models.length > 0 && <span className="text-[12px] text-faint">(limited to these)</span>}
                  {models.length === 0 && !h.models_refreshing && <span className="text-faint">{h.ready ? "No model list yet" : "Listed once it has a credential"}</span>}
                  {h.models_refreshing ? (
                    <span className="flex items-center gap-1.5 text-muted"><RefreshCw className="size-3.5 animate-spin" /> Asking the harness which models it offers</span>
                  ) : h.ready && (
                    <Button variant="ghost" size="sm" disabled={refresh.busy} onClick={() => void refreshModels(h)}><RefreshCw /> Refresh</Button>
                  )}
                </div>
                {h.models_error && !h.models_refreshing && <Notice tone="error" title="Could not list its models">{h.models_error}</Notice>}
                {h.credentials.length > 0 && (
                  <>
                    <CredentialLine label="Team credential" value={h.team_credential} canEdit={isAdmin} onEdit={() => setCredential({ harness: h, scope: "team" })} />
                    <CredentialLine label="Mine" value={h.my_credential} canEdit onEdit={() => setCredential({ harness: h, scope: "me" })} />
                  </>
                )}
              </div>
            </Card>
          );
        })}
      </div>
      <p className="mt-4 max-w-[80ch] text-[12.5px] text-muted">
        When you have your own credential for a harness, your agents use it; otherwise they use the team’s. Credentials are encrypted at rest and delivered only to the sandbox of a session that needs them.
      </p>
      {credential && <CredentialDialog {...credential} onClose={() => (setCredential(null), reload())} />}
      {editing && <HarnessDialog harness={editing === "new" ? null : editing} onClose={() => (setEditing(null), reload())} />}
    </>
  );
}

function CredentialLine(props: { label: string; value: string | null; canEdit: boolean; onEdit: () => void }) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <span className="w-28 shrink-0 text-muted">{props.label}</span>
      {props.value ? <Badge tone="run" mono><KeyRound /> {props.value}</Badge> : <span className="text-faint">None</span>}
      {props.canEdit && <Button variant="ghost" size="sm" onClick={props.onEdit}>{props.value ? "Replace" : <><Plus /> Add</>}</Button>}
    </div>
  );
}

function CredentialDialog({ harness, scope, onClose }: { harness: Harness; scope: "team" | "me"; onClose: () => void }) {
  // Subscription tokens are personal; they cannot be a team credential.
  const slots: CredentialSlot[] = harness.credentials.filter((s) => scope === "me" || s.kind === "api_key");
  const current = scope === "team" ? harness.team_credential : harness.my_credential;
  const [env, setEnv] = useState(current ?? slots[0]?.env ?? "");
  const [value, setValue] = useState("");
  const action = useAction();
  const slot = slots.find((s) => s.env === env);
  const save = () => value.trim() && env && action.run(async () => (await api.put(`/harnesses/${harness.id}/credential`, { scope, env, value }), onClose()), { success: "Credential saved. New sessions will use it." });
  const remove = async () => {
    const ok = await confirm({
      title: "Remove this credential?",
      description: scope === "team" ? "Sessions that rely on the team credential will not be able to start until someone adds one." : "Your new sessions will fall back to the team credential, if there is one.",
      confirmLabel: "Remove",
      destructive: true,
    });
    if (ok) void action.run(async () => (await api.del(`/harnesses/${harness.id}/credential?scope=${scope}`), onClose()), { success: "Credential removed" });
  };
  return (
    <Modal
      title={`${scope === "team" ? "Team" : "My"} credential for ${harness.title}`}
      description={scope === "team" ? "Used by everyone’s agents that do not have their own." : "Used by your agents only, instead of the team’s."}
      onClose={onClose}
      footer={
        <>
          {current && <Button variant="danger-ghost" className="mr-auto" disabled={action.busy} onClick={() => void remove()}>Remove</Button>}
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!value.trim() || !env} loading={action.busy} onClick={() => void save()}>Save</Button>
        </>
      }
    >
      {slots.length > 1 && (
        <Field label="Type">{(id) => <Select id={id} value={env} onValueChange={setEnv} options={slots.map((s) => ({ value: s.env, label: s.label, description: s.kind === "oauth" ? "Personal subscription" : "API key" }))} />}</Field>
      )}
      {slot?.kind === "oauth" && (
        <Notice tone="warning" title="This uses your personal subscription">
          It is for your own sessions only, rather than pay-as-you-go API billing. Whether a subscription may be used this way is up to your provider’s terms, which have changed more than once. For a team, use an API key.
        </Notice>
      )}
      <Field label={slot?.label ?? "Credential"} hint={<>Set as <code className="font-mono text-[12px] text-foreground">{env}</code> in the sandbox. Stored encrypted; never shown again.</>}>
        {(id) => <Input id={id} type="password" mono autoFocus autoComplete="off" value={value} onChange={(e) => setValue(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void save()} />}
      </Field>
      <ErrorNote error={action.error} />
    </Modal>
  );
}

function HarnessDialog({ harness, onClose }: { harness: Harness | null; onClose: () => void }) {
  const [name, setName] = useState(harness?.name ?? "");
  const [title, setTitle] = useState(harness?.title ?? "");
  const [description, setDescription] = useState(harness?.description ?? "");
  const [image, setImage] = useState(harness?.image ?? "");
  const [command, setCommand] = useState(harness ? JSON.stringify(harness.command) : '["my-agent", "--acp"]');
  const [credentials, setCredentials] = useState(harness ? harness.credentials.map((c) => `${c.env}=${c.label}${c.kind === "oauth" ? " [oauth]" : ""}`).join("\n") : "");
  const [egress, setEgress] = useState(harness?.egress_hosts.join("\n") ?? "");
  const [defaultModel, setDefaultModel] = useState(harness?.default_model ?? "");
  const [models, setModels] = useState(harness?.models.map((m) => (m.label ? `${m.id}=${m.label}` : m.id)).join("\n") ?? "");
  const [modelEnv, setModelEnv] = useState(harness?.model_env ?? "");
  const discovered = harness?.discovered_models ?? [];
  const action = useAction();

  const save = () =>
    action.run(async () => {
      let argv: unknown;
      try {
        argv = JSON.parse(command);
      } catch {
        throw new Error('The command must be a JSON array, for example ["my-agent", "--acp"].');
      }
      const slots = credentials.split("\n").map((l) => l.trim()).filter(Boolean).map((line) => {
        const oauth = line.endsWith("[oauth]");
        const [env, ...rest] = line.replace(/\s*\[oauth\]$/, "").split("=");
        return { env: env.trim(), label: rest.join("=").trim() || env.trim(), kind: oauth ? "oauth" : "api_key" };
      });
      const allowed = models.split("\n").map((l) => l.trim()).filter(Boolean).map((line) => {
        const [id, ...rest] = line.split("=");
        const known = discovered.find((m) => m.id === id.trim());
        return { id: id.trim(), label: rest.join("=").trim() || known?.label || "", description: known?.description ?? "" };
      });
      const body = {
        name, title, description, image, command: argv, credentials: slots,
        egress_hosts: egress.split("\n").map((l) => l.trim()).filter(Boolean),
        models: allowed,
        default_model: defaultModel.trim() || null,
        model_env: modelEnv.trim() || null,
      };
      await (harness ? api.patch(`/harnesses/${harness.id}`, body) : api.post("/harnesses", body));
      onClose();
    }, { success: harness ? "Harness saved" : "Harness added" });
  const remove = async () => {
    const ok = await confirm({ title: `Delete ${harness!.title}?`, description: "Its stored credentials are deleted too, for everyone. Workflows that name it will stop working.", confirmLabel: "Delete harness", destructive: true });
    if (ok) void action.run(async () => (await api.del(`/harnesses/${harness!.id}`), onClose()), { success: "Harness deleted" });
  };
  const hasChoices = discovered.length > 0 || (harness?.models.length ?? 0) > 0;

  return (
    <Modal
      title={harness ? `Edit ${harness.title}` : "Add a harness"}
      onClose={onClose}
      size="lg"
      modalOnly
      footer={
        <>
          {harness && !harness.builtin && <Button variant="danger-ghost" className="mr-auto" disabled={action.busy} onClick={() => void remove()}><Trash2 /> Delete</Button>}
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" loading={action.busy} onClick={() => void save()}>Save</Button>
        </>
      }
    >
      <div className="grid gap-4 sm:grid-cols-2">
        {!harness && <Field label="Name" hint="Lowercase letters, digits and dashes. Used in workflows.">{(id) => <Input id={id} mono value={name} onChange={(e) => setName(e.target.value)} placeholder="my-agent" />}</Field>}
        <Field label="Title" className={harness ? "sm:col-span-2" : undefined}>{(id) => <Input id={id} value={title} onChange={(e) => setTitle(e.target.value)} />}</Field>
      </div>
      <Field label="Description">{(id) => <Input id={id} value={description} onChange={(e) => setDescription(e.target.value)} />}</Field>
      <Field label="Container image" hint="Needs git and the agent itself. Hyperbeam adds its runner at start-up.">{(id) => <Input id={id} mono value={image} onChange={(e) => setImage(e.target.value)} placeholder="ghcr.io/acme/my-agent:1.2" />}</Field>
      <Field label="Command" hint="JSON array that starts the agent speaking the Agent Client Protocol on stdio.">{(id) => <Input id={id} mono value={command} onChange={(e) => setCommand(e.target.value)} />}</Field>
      <div className="grid gap-4 sm:grid-cols-2">
        <Field label="Default model" hint="Used when someone starts work without choosing a model.">
          {(id) =>
            hasChoices ? (
              <Select
                id={id}
                value={defaultModel}
                onValueChange={setDefaultModel}
                options={[
                  { value: "", label: "The harness’s own default" },
                  ...(harness?.model_choices ?? []).map((m) => ({ value: m.id, label: m.label || m.id })),
                  ...(defaultModel && !(harness?.model_choices ?? []).some((m) => m.id === defaultModel) ? [{ value: defaultModel, label: defaultModel }] : []),
                ]}
              />
            ) : (
              <Input id={id} mono value={defaultModel} onChange={(e) => setDefaultModel(e.target.value)} placeholder="The harness’s own default" />
            )
          }
        </Field>
        <Field label="Model environment variable" optional hint="Also set to the chosen model, for agents that read it from the environment.">
          {(id) => <Input id={id} mono value={modelEnv} onChange={(e) => setModelEnv(e.target.value)} placeholder="ANTHROPIC_MODEL" />}
        </Field>
      </div>
      <Field
        label="Models people may choose"
        optional
        hint={<>One model id per line, optionally as id=Label. Listing any restricts the choice to these.{discovered.length > 0 && <> This harness offers: {discovered.map((m) => m.id).join(", ")}.</>}</>}
      >
        {(id) => <Textarea id={id} mono rows={3} className="min-h-0" value={models} onChange={(e) => setModels(e.target.value)} placeholder="Leave empty to allow every model the harness offers" />}
      </Field>
      <Field label="Credentials it accepts" hint="One per line as ENV_NAME=Label. Add [oauth] at the end for a personal subscription token.">
        {(id) => <Textarea id={id} mono rows={3} className="min-h-0" value={credentials} onChange={(e) => setCredentials(e.target.value)} placeholder="MY_API_KEY=My provider API key" />}
      </Field>
      <Field label="Hosts it may reach" hint="One per line. Enforced when the egress proxy is enabled.">
        {(id) => <Textarea id={id} mono rows={3} className="min-h-0" value={egress} onChange={(e) => setEgress(e.target.value)} placeholder={"api.example.com\n*.example.com"} />}
      </Field>
      <ErrorNote error={action.error} />
    </Modal>
  );
}

// ------------------------------------------------------------------- general

function General() {
  const { data, error, reload } = useLoad<any>(() => api.get("/settings"), []);
  const harnesses = useLoad<{ harnesses: Harness[] }>(() => api.get("/harnesses"), []);
  const [form, setForm] = useState<{ team_name: string; default_harness: string; egress: string; web_mode: string; web_domains: string } | null>(null);
  const action = useAction();
  if (data && !form) {
    setForm({
      team_name: data.team_name ?? "",
      default_harness: data.default_harness,
      egress: (data.egress_allow as string[]).join("\n"),
      web_mode: data.web_access.mode,
      web_domains: (data.web_access.domains as string[]).join("\n"),
    });
  }
  if (error) return <ErrorNote error={error} />;
  if (!data || !form) return <div className="grid max-w-[600px] gap-4"><Skeleton className="h-14" /><Skeleton className="h-14" /><Skeleton className="h-28" /></div>;
  const dirty =
    form.team_name !== (data.team_name ?? "") ||
    form.default_harness !== data.default_harness ||
    form.egress !== (data.egress_allow as string[]).join("\n") ||
    form.web_mode !== data.web_access.mode ||
    form.web_domains !== (data.web_access.domains as string[]).join("\n");
  const lines = (text: string) => text.split("\n").map((l) => l.trim()).filter(Boolean);
  const save = () =>
    action.run(async () => {
      await api.put("/settings", {
        team_name: form.team_name,
        default_harness: form.default_harness,
        egress_allow: lines(form.egress),
        web_access: { mode: form.web_mode, domains: lines(form.web_domains) },
      });
      reload();
      setForm(null);
    }, { success: "Settings saved" });
  return (
    <form className="grid max-w-[600px] gap-5" onSubmit={(e) => (e.preventDefault(), void save())}>
      <Field label="Team name">{(id) => <Input id={id} value={form.team_name} onChange={(e) => setForm({ ...form, team_name: e.target.value })} />}</Field>
      <Field label="Default harness" hint="Preselected in New work, and used by workflows that do not name one.">
        {(id) => <Select id={id} value={form.default_harness} onValueChange={(v) => setForm({ ...form, default_harness: v })} options={harnesses.data?.harnesses.map((h) => ({ value: h.name, label: h.title })) ?? []} />}
      </Field>
      <Field
        label="Extra hosts every sandbox may reach"
        hint={data.egress_enforced
          ? <>Sandboxes reach the internet only through Hyperbeam’s allowlist. Always allowed: {data.egress_allow_builtin.join(", ")}, plus each harness’s own hosts.</>
          : undefined}
      >
        {(id) => <Textarea id={id} mono rows={4} value={form.egress} onChange={(e) => setForm({ ...form, egress: e.target.value })} placeholder="internal-registry.example.com" />}
      </Field>
      {!data.egress_enforced && (
        <Notice tone="warning" title="Network access is not restricted">The egress proxy is not enabled on this installation, so Hyperbeam does not limit where sandboxes connect. This list takes effect once it is.</Notice>
      )}
      <Field
        label="Web pages agents may read"
        hint="Agents read the web through Hyperbeam’s web_fetch tool, not from their sandbox, and only if the person who started them holds “Read public web pages”. Every fetch is recorded."
      >
        {(id) => (
          <Select
            id={id}
            value={form.web_mode}
            onValueChange={(v) => setForm({ ...form, web_mode: v })}
            options={[
              { value: "known", label: "Pages they were given or found, and the sites listed below" },
              { value: "listed", label: "Only the sites listed below" },
              { value: "off", label: "None" },
            ]}
          />
        )}
      </Field>
      {form.web_mode !== "off" && (
        <Field
          label="Sites agents may read freely"
          optional={form.web_mode === "known"}
          hint={form.web_mode === "known"
            ? "One per line. Elsewhere, agents can fetch only URLs a person gave them, a web search returned, or a fetched page links to, so they cannot send data out in a URL they wrote. On these sites they can fetch any URL."
            : "One per line. Agents can fetch any URL on these sites and nothing else."}
        >
          {(id) => <Textarea id={id} mono rows={3} value={form.web_domains} onChange={(e) => setForm({ ...form, web_domains: e.target.value })} placeholder={"docs.rs\n*.readthedocs.io"} />}
        </Field>
      )}
      <ErrorNote error={action.error} />
      <div className="flex items-center gap-3">
        <Button type="submit" variant="primary" disabled={!dirty} loading={action.busy}>Save changes</Button>
        {dirty && <Button variant="ghost" onClick={() => setForm(null)}>Discard</Button>}
      </div>
      <p className="border-t border-border pt-4 text-[12.5px] text-muted">
        Idle sandboxes are torn down after {Math.round(data.idle_timeout_secs / 60)} minutes and at most {data.max_sessions} run at once. Both are set when the server starts.
      </p>
    </form>
  );
}

// --------------------------------------------------------------------- audit

interface AuditRow {
  id: number;
  ts: string;
  session_id: string | null;
  session_title: string | null;
  user_id: string | null;
  action: string;
  target: string;
  allowed: boolean;
  detail: string;
}

function Audit() {
  const [deniedOnly, setDeniedOnly] = useState(false);
  const users = useStore((s) => s.users);
  const { data, error } = useLoad<{ audit: AuditRow[] }>(() => api.get(`/audit?limit=300${deniedOnly ? "&denied=true" : ""}`), [deniedOnly]);
  return (
    <>
      <Lead
        action={
          <label className="flex cursor-pointer items-center gap-2 text-[13px] font-medium">
            <Switch checked={deniedOnly} onCheckedChange={setDeniedOnly} aria-label="Blocked only" /> Blocked only
          </label>
        }
      >
        Every attempt by an agent to act outside its sandbox: tool calls, git operations and blocked network requests.
      </Lead>
      <ErrorNote error={error} />
      {!data && !error && <Skeleton className="h-64 rounded-lg" />}
      {data && data.audit.length === 0 && (
        <EmptyState icon={<ScrollText />} title={deniedOnly ? "Nothing blocked" : "Nothing recorded yet"}>
          {deniedOnly ? "No agent has tried to do something it was not allowed to." : "Actions agents take outside their sandbox show up here."}
        </EmptyState>
      )}
      {data && data.audit.length > 0 && (
        <Card className="overflow-x-auto">
          <table className="w-full min-w-[820px] border-collapse text-[13px]">
            <thead>
              <tr className="border-b border-border text-left text-[12px] text-muted">
                {["When", "Agent", "On behalf of", "Action", "Target", "Outcome"].map((h) => <th key={h} className="px-4 py-2.5 font-medium">{h}</th>)}
              </tr>
            </thead>
            <tbody className="divide-y divide-border">
              {data.audit.map((row) => (
                <tr key={row.id} className="hover:bg-surface-2/40">
                  <td className="px-4 py-2 text-muted"><Time iso={row.ts} /></td>
                  <td className="max-w-56 truncate px-4 py-2">{row.session_id ? <A href={`/s/${row.session_id}`} className="hover:text-primary">{row.session_title ?? "Session"}</A> : <span className="text-faint">None</span>}</td>
                  <td className="px-4 py-2">{row.user_id ? users[row.user_id]?.name ?? "Unknown" : <span className="text-faint">None</span>}</td>
                  <td className="px-4 py-2 font-mono text-[12px]">{row.action}</td>
                  <td className="max-w-64 truncate px-4 py-2 font-mono text-[12px]" title={row.target}>{row.target}</td>
                  <td className="px-4 py-2" title={row.detail || undefined}>
                    {row.allowed ? (row.detail ? <Badge tone="wait">Failed</Badge> : <Badge tone="run">Allowed</Badge>) : <Badge tone="fail">Blocked</Badge>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      )}
    </>
  );
}

// ------------------------------------------------------------------- account

interface ApiKey {
  id: string;
  name: string;
  created_at: string;
  last_used_at: string | null;
}

function AccountSection({ title, description, children }: { title: string; description?: ReactNode; children: ReactNode }) {
  return (
    <section className="grid gap-x-10 gap-y-3 border-b border-border py-7 first:pt-0 last:border-0 md:grid-cols-[220px_minmax(0,1fr)]">
      <div>
        <h2 className="font-display text-[15px] font-semibold">{title}</h2>
        {description && <p className="mt-0.5 text-[13px] text-muted">{description}</p>}
      </div>
      <div className="grid max-w-[520px] gap-4">{children}</div>
    </section>
  );
}

function Account() {
  const me = useStore((s) => s.me)!;
  const keys = useLoad<{ api_keys: ApiKey[] }>(() => api.get("/me/api-keys"), []);
  const [name, setName] = useState(me.name);
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [keyName, setKeyName] = useState("");
  const [newKey, setNewKey] = useState<string | null>(null);
  const profile = useAction();
  const password = useAction();
  const keyAction = useAction();

  const saveName = () => profile.run(async () => (await api.patch("/me", { name }), await refreshMe()), { success: "Name updated" });
  const savePassword = () => password.run(async () => (await api.patch("/me", { current_password: current, new_password: next }), setCurrent(""), setNext("")), { success: "Password changed" });
  const createKey = () => keyName.trim() && keyAction.run(async () => (setNewKey((await api.post("/me/api-keys", { name: keyName })).token), setKeyName(""), keys.reload()));
  const revoke = async (key: ApiKey) => {
    const ok = await confirm({ title: `Revoke “${key.name}”?`, description: "Anything using this key stops working straight away.", confirmLabel: "Revoke key", destructive: true });
    if (ok) void keyAction.run(async () => (await api.del(`/me/api-keys/${key.id}`), keys.reload()), { success: "Key revoked", toastError: true });
  };

  return (
    <div className="grid">
      <AccountSection title="Profile" description={me.email}>
        <form className="flex gap-2" onSubmit={(e) => (e.preventDefault(), void saveName())}>
          <Input aria-label="Name" value={name} onChange={(e) => setName(e.target.value)} />
          <Button type="submit" disabled={!name.trim() || name === me.name} loading={profile.busy}>Save</Button>
        </form>
        <ErrorNote error={profile.error} />
      </AccountSection>

      <AccountSection title="Password">
        <form className="grid gap-4" onSubmit={(e) => (e.preventDefault(), void savePassword())}>
          <Field label="Current password">{(id) => <Input id={id} type="password" autoComplete="current-password" value={current} onChange={(e) => setCurrent(e.target.value)} />}</Field>
          <Field label="New password" hint="At least 10 characters.">{(id) => <Input id={id} type="password" autoComplete="new-password" value={next} onChange={(e) => setNext(e.target.value)} />}</Field>
          <ErrorNote error={password.error} />
          <div><Button type="submit" disabled={!current || next.length < 10} loading={password.busy}>Change password</Button></div>
        </form>
      </AccountSection>

      <AccountSection title="API keys" description={<>For the <code className="font-mono text-[12px]">hyperbeam</code> command-line tool and your own scripts. A key acts as you.</>}>
        {newKey && (
          <Notice tone="success" title="Copy this key now. It will not be shown again.">
            <div className="mt-2"><CopyField value={newKey} /></div>
          </Notice>
        )}
        {keys.data && keys.data.api_keys.length > 0 && (
          <Card className="divide-y divide-border">
            {keys.data.api_keys.map((key) => (
              <div key={key.id} className="flex items-center gap-3 px-3.5 py-2.5">
                <KeyRound className="size-4 shrink-0 text-faint" />
                <div className="min-w-0 flex-1">
                  <div className="font-medium">{key.name}</div>
                  <div className="text-[12px] text-muted">Created <Time iso={key.created_at} /> · {key.last_used_at ? <>last used <Time iso={key.last_used_at} /></> : "never used"}</div>
                </div>
                <Button variant="danger-ghost" size="sm" onClick={() => void revoke(key)}>Revoke</Button>
              </div>
            ))}
          </Card>
        )}
        <form className="flex gap-2" onSubmit={(e) => (e.preventDefault(), void createKey())}>
          <Input aria-label="Key name" placeholder="Key name, e.g. laptop" value={keyName} onChange={(e) => setKeyName(e.target.value)} />
          <Button type="submit" disabled={!keyName.trim()} loading={keyAction.busy}><Plus /> Create key</Button>
        </form>
        <ErrorNote error={keyAction.error} />
      </AccountSection>
    </div>
  );
}

