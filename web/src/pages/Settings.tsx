import { useEffect, useState } from "react";
import { api, type CredentialSlot, type Harness, modelLabel, type Repo } from "../lib/api";
import { refreshMe, useStore } from "../lib/store";
import { A, Empty, ErrorNote, Modal, navigate, timeAgo, useAction, useLoad } from "../lib/ui";

const TABS: [string, string, boolean][] = [
  ["integrations", "Integrations", false],
  ["repositories", "Repositories", false],
  ["harnesses", "Harnesses", false],
  ["general", "General", true],
  ["audit", "Audit log", true],
  ["account", "My account", false],
];

export function SettingsPage({ tab }: { tab: string }) {
  const me = useStore((s) => s.me)!;
  const isAdmin = me.role !== "member";
  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Settings</h1>
          <p>Connect the systems your agents work with and choose how they run.</p>
        </div>
      </div>
      <div className="tabs">
        {TABS.filter(([, , adminOnly]) => isAdmin || !adminOnly).map(([id, label]) => (
          <button key={id} className={tab === id ? "active" : ""} onClick={() => navigate(`/settings/${id}`)}>{label}</button>
        ))}
      </div>
      {tab === "repositories" ? <Repositories isAdmin={isAdmin} /> :
       tab === "harnesses" ? <Harnesses isAdmin={isAdmin} /> :
       tab === "general" && isAdmin ? <General /> :
       tab === "audit" && isAdmin ? <Audit /> :
       tab === "account" ? <Account /> :
       <Integrations isAdmin={isAdmin} />}
    </div>
  );
}

// -------------------------------------------------------------- integrations

interface Integration {
  kind: "github" | "linear" | "notion";
  connected: boolean;
  config: { mode?: string; app_id?: string };
  updated_at?: string;
}

const INTEGRATION_COPY: Record<string, { title: string; blurb: string }> = {
  github: { title: "GitHub", blurb: "Lets agents clone and push to registered repositories, and work with issues and pull requests." },
  linear: { title: "Linear", blurb: "Lets agents read, create and update issues." },
  notion: { title: "Notion", blurb: "Lets agents read and write the pages you share with the Noemune integration." },
};

function Integrations({ isAdmin }: { isAdmin: boolean }) {
  const { data, error, reload } = useLoad<{ integrations: Integration[] }>(() => api.get("/integrations"), []);
  const [editing, setEditing] = useState<Integration | null>(null);
  const [results, setResults] = useState<Record<string, { ok: boolean; detail: string }>>({});
  const action = useAction();
  const test = (kind: string) =>
    action.run(async () => {
      const result = await api.post(`/integrations/${kind}/test`);
      setResults((r) => ({ ...r, [kind]: result }));
    });
  return (
    <div className="section">
      <p className="lead">
        These credentials stay in Noemune’s control plane. Agents never see them: every request an agent makes goes through Noemune, which checks it against the permissions of the person who started the agent.
      </p>
      <ErrorNote error={error ?? action.error} />
      <div className="stack">
        {data?.integrations.map((integration) => {
          const copy = INTEGRATION_COPY[integration.kind];
          const result = results[integration.kind];
          return (
            <div key={integration.kind} className="card card-pad">
              <div className="spread">
                <div>
                  <div className="row">
                    <h2>{copy.title}</h2>
                    {integration.connected ? <span className="chip chip-green">Connected</span> : <span className="chip">Not connected</span>}
                    {integration.connected && integration.kind === "github" && (
                      <span className="chip">{integration.config.mode === "app" ? `GitHub App ${integration.config.app_id}` : "Access token"}</span>
                    )}
                  </div>
                  <p className="muted" style={{ margin: "4px 0 0" }}>{copy.blurb}</p>
                </div>
                {isAdmin && (
                  <div className="row">
                    {integration.connected && <button className="btn" disabled={action.busy} onClick={() => void test(integration.kind)}>Test</button>}
                    <button className="btn" onClick={() => setEditing(integration)}>{integration.connected ? "Change" : "Connect"}</button>
                  </div>
                )}
              </div>
              {result && <div className={result.ok ? "ok-note" : "error-note"} style={{ marginTop: 10 }}>{result.detail}</div>}
            </div>
          );
        })}
      </div>
      {editing && <IntegrationDialog integration={editing} onClose={() => (setEditing(null), reload())} />}
    </div>
  );
}

function IntegrationDialog({ integration, onClose }: { integration: Integration; onClose: () => void }) {
  const [mode, setMode] = useState(integration.config.mode ?? "token");
  const [appId, setAppId] = useState(integration.config.app_id ?? "");
  const [secret, setSecret] = useState("");
  const action = useAction();
  const { kind } = integration;
  const save = () =>
    action.run(async () => {
      await api.put(`/integrations/${kind}`, { config: kind === "github" ? { mode, app_id: appId } : {}, secret: secret || undefined });
      onClose();
    });
  const disconnect = () => action.run(async () => (await api.del(`/integrations/${kind}`), onClose()));
  const isApp = kind === "github" && mode === "app";
  return (
    <Modal title={`${integration.connected ? "Change" : "Connect"} ${INTEGRATION_COPY[kind].title}`} onClose={onClose}>
      {kind === "github" && (
        <>
          <label className="field">
            How Noemune authenticates
            <select value={mode} onChange={(e) => setMode(e.target.value)}>
              <option value="token">Access token: simplest, good for one person or a small team</option>
              <option value="app">GitHub App: recommended for organisations</option>
            </select>
          </label>
          {isApp ? (
            <p className="small muted" style={{ margin: 0 }}>
              Create a GitHub App with repository permissions Contents, Pull requests and Issues set to read &amp; write, install it on the repositories you will register, then paste its App ID and a generated private key. Noemune mints a short-lived token per repository for each request.
            </p>
          ) : (
            <p className="small muted" style={{ margin: 0 }}>
              A fine-grained personal access token limited to the repositories you will register, with Contents, Pull requests and Issues set to read &amp; write. Everything agents do on GitHub will appear as this token’s user.
            </p>
          )}
          {isApp && <label className="field">App ID<input type="text" value={appId} onChange={(e) => setAppId(e.target.value)} /></label>}
        </>
      )}
      {kind === "linear" && <p className="small muted" style={{ margin: 0 }}>A Linear API key (Settings → Security &amp; access → Personal API keys), or an OAuth access token. Agents act as this key’s user.</p>}
      {kind === "notion" && <p className="small muted" style={{ margin: 0 }}>The secret of a Notion internal integration. In Notion, share each page agents should reach with that integration.</p>}
      <label className="field">
        {isApp ? "Private key (PEM)" : kind === "notion" ? "Integration secret" : kind === "linear" ? "API key" : "Token"}
        {isApp ? (
          <textarea className="code" rows={6} value={secret} onChange={(e) => setSecret(e.target.value)} placeholder="-----BEGIN RSA PRIVATE KEY-----" />
        ) : (
          <input type="password" autoComplete="off" value={secret} onChange={(e) => setSecret(e.target.value)} />
        )}
        {integration.connected && <span className="hint">Leave blank to keep the current one.</span>}
      </label>
      <ErrorNote error={action.error} />
      <div className="modal-foot">
        {integration.connected && <button className="btn btn-danger" style={{ marginRight: "auto" }} disabled={action.busy} onClick={() => void disconnect()}>Disconnect</button>}
        <button className="btn" onClick={onClose}>Cancel</button>
        <button className="btn btn-primary" disabled={action.busy || (!integration.connected && !secret.trim())} onClick={() => void save()}>Save</button>
      </div>
    </Modal>
  );
}

// -------------------------------------------------------------- repositories

function Repositories({ isAdmin }: { isAdmin: boolean }) {
  const { data, error, reload } = useLoad<{ repos: Repo[] }>(() => api.get("/repos"), []);
  const [name, setName] = useState("");
  const action = useAction();
  const add = () => action.run(async () => (await api.post("/repos", { full_name: name }), setName(""), reload()));
  return (
    <div className="section">
      <p className="lead">Agents can only reach repositories registered here, and only the ones their owner has access to (see <A href="/team">People &amp; permissions</A>).</p>
      {isAdmin && (
        <div className="row" style={{ marginBottom: 14, maxWidth: 520 }}>
          <input type="text" placeholder="owner/name" value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && name.trim() && void add()} />
          <button className="btn btn-primary" disabled={!name.trim() || action.busy} onClick={() => void add()}>{action.busy ? "Checking…" : "Register"}</button>
        </div>
      )}
      <ErrorNote error={error ?? action.error} />
      {data && data.repos.length === 0 && <Empty title="No repositories yet">{isAdmin ? "Connect GitHub under Integrations, then register a repository as owner/name." : "Ask an admin to register a repository."}</Empty>}
      {data && data.repos.length > 0 && (
        <div className="card">
          <table>
            <thead><tr><th>Repository</th><th>Default branch</th><th>Your access</th><th /></tr></thead>
            <tbody>
              {data.repos.map((repo) => (
                <tr key={repo.id}>
                  <td><strong>{repo.full_name}</strong>{repo.description && <div className="small muted">{repo.description}</div>}</td>
                  <td className="mono">{repo.default_branch}</td>
                  <td>{repo.accessible ? <span className="chip chip-green">Yes</span> : <span className="chip">No</span>}</td>
                  <td style={{ textAlign: "right" }}>
                    {isAdmin && <button className="btn btn-sm btn-ghost" onClick={() => void action.run(async () => (await api.del(`/repos/${repo.id}`), reload()))}>Remove</button>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
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
  const refreshModels = (h: Harness) => refresh.run(async () => (await api.post(`/harnesses/${h.id}/models/refresh`, {}), reload()));
  return (
    <div className="section">
      <div className="spread" style={{ marginBottom: 12 }}>
        <p className="lead" style={{ margin: 0 }}>
          A harness is the coding agent that runs inside each sandbox. Noemune drives any agent that speaks the Agent Client Protocol. Give each one a credential: a team API key, or your own.
        </p>
        {isAdmin && <button className="btn" onClick={() => setEditing("new")}>Add a harness</button>}
      </div>
      <ErrorNote error={error ?? refresh.error} />
      <div className="stack">
        {data?.harnesses.map((h) => (
          <div key={h.id} className="card card-pad">
            <div className="spread">
              <div>
                <div className="row">
                  <h2>{h.title}</h2>
                  {h.ready ? <span className="chip chip-green">Ready</span> : <span className="chip">Needs a credential</span>}
                </div>
                <p className="muted" style={{ margin: "4px 0 0" }}>{h.description}</p>
                <div className="small faint mono" style={{ marginTop: 6 }}>{h.image} · {h.command.join(" ")}</div>
                <div className="row-wrap small" style={{ marginTop: 8 }}>
                  <span className="muted">Default model:</span>
                  <span className="chip">{modelLabel(h.model_choices, h.default_model) ?? "the harness’s own"}</span>
                  {h.models.length > 0 ? (
                    <span className="muted">· limited to {h.models.map((m) => m.label || m.id).join(", ")}</span>
                  ) : h.discovered_models.length > 0 ? (
                    <span className="muted">· offers {h.discovered_models.map((m) => m.label || m.id).join(", ")}</span>
                  ) : h.models_refreshing ? null : (
                    <span className="faint">· {h.ready ? "no model list yet" : "models are listed once it has a credential"}</span>
                  )}
                  {h.models_refreshing ? (
                    <span className="muted">· asking the harness which models it offers…</span>
                  ) : h.ready && (
                    <button className="btn btn-sm" disabled={refresh.busy} onClick={() => void refreshModels(h)}>Refresh models</button>
                  )}
                </div>
                {h.models_error && !h.models_refreshing && (
                  <div className="small" style={{ marginTop: 6, color: "var(--red)", whiteSpace: "pre-wrap" }}>Could not list its models: {h.models_error}</div>
                )}
              </div>
              {isAdmin && <button className="btn btn-sm" onClick={() => setEditing(h)}>Edit</button>}
            </div>
            {h.credentials.length > 0 && (
              <div className="row-wrap" style={{ marginTop: 12, gap: 18 }}>
                <div className="row">
                  <span className="muted">Team credential:</span>
                  {h.team_credential ? <span className="chip chip-green mono">{h.team_credential}</span> : <span className="faint">none</span>}
                  {isAdmin && <button className="btn btn-sm" onClick={() => setCredential({ harness: h, scope: "team" })}>{h.team_credential ? "Replace" : "Add"}</button>}
                </div>
                <div className="row">
                  <span className="muted">Mine:</span>
                  {h.my_credential ? <span className="chip chip-green mono">{h.my_credential}</span> : <span className="faint">none</span>}
                  <button className="btn btn-sm" onClick={() => setCredential({ harness: h, scope: "me" })}>{h.my_credential ? "Replace" : "Add"}</button>
                </div>
              </div>
            )}
          </div>
        ))}
      </div>
      <p className="small muted" style={{ marginTop: 12, maxWidth: "80ch" }}>
        When you have your own credential for a harness, your agents use it; otherwise they use the team’s. Credentials are encrypted at rest and delivered only to the sandbox of a session that needs them.
      </p>
      {credential && <CredentialDialog {...credential} onClose={() => (setCredential(null), reload())} />}
      {editing && <HarnessDialog harness={editing === "new" ? null : editing} onClose={() => (setEditing(null), reload())} />}
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
  const save = () => action.run(async () => (await api.put(`/harnesses/${harness.id}/credential`, { scope, env, value }), onClose()));
  const remove = () => action.run(async () => (await api.del(`/harnesses/${harness.id}/credential?scope=${scope}`), onClose()));
  return (
    <Modal title={`${scope === "team" ? "Team" : "My"} credential for ${harness.title}`} onClose={onClose}>
      {slots.length > 1 && (
        <label className="field">
          Type
          <select value={env} onChange={(e) => setEnv(e.target.value)}>
            {slots.map((s) => <option key={s.env} value={s.env}>{s.label}</option>)}
          </select>
        </label>
      )}
      {slot?.kind === "oauth" && (
        <div className="warn-note">
          This uses your personal subscription rather than pay-as-you-go API billing. It is for your own sessions only, and whether a subscription may be used this way is up to your provider’s terms, which have changed more than once. For a team, use an API key.
        </div>
      )}
      <label className="field">
        {slot?.label ?? "Credential"}
        <input type="password" autoFocus autoComplete="off" value={value} onChange={(e) => setValue(e.target.value)} />
        <span className="hint">Set as <code>{env}</code> in the sandbox. Stored encrypted; never shown again.</span>
      </label>
      <ErrorNote error={action.error} />
      <div className="modal-foot">
        {current && <button className="btn btn-danger" style={{ marginRight: "auto" }} disabled={action.busy} onClick={() => void remove()}>Remove</button>}
        <button className="btn" onClick={onClose}>Cancel</button>
        <button className="btn btn-primary" disabled={!value.trim() || !env || action.busy} onClick={() => void save()}>Save</button>
      </div>
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
    });
  const remove = () => action.run(async () => (await api.del(`/harnesses/${harness!.id}`), onClose()));

  return (
    <Modal title={harness ? `Edit ${harness.title}` : "Add a harness"} onClose={onClose} wide>
      {!harness && <label className="field">Name<input type="text" value={name} onChange={(e) => setName(e.target.value)} placeholder="my-agent" /><span className="hint">Lowercase letters, digits and dashes. Used in workflows.</span></label>}
      <label className="field">Title<input type="text" value={title} onChange={(e) => setTitle(e.target.value)} /></label>
      <label className="field">Description<input type="text" value={description} onChange={(e) => setDescription(e.target.value)} /></label>
      <label className="field">Container image<input type="text" className="mono" value={image} onChange={(e) => setImage(e.target.value)} placeholder="ghcr.io/acme/my-agent:1.2" /><span className="hint">Needs git and the agent itself. Noemune adds its runner at start-up.</span></label>
      <label className="field">Command<input type="text" className="mono" value={command} onChange={(e) => setCommand(e.target.value)} /><span className="hint">JSON array that starts the agent speaking the Agent Client Protocol on stdio.</span></label>
      <div className="row" style={{ alignItems: "flex-start" }}>
        <label className="field grow">
          Default model
          {discovered.length > 0 || (harness?.models.length ?? 0) > 0 ? (
            <select value={defaultModel} onChange={(e) => setDefaultModel(e.target.value)}>
              <option value="">The harness’s own default</option>
              {(harness?.model_choices ?? []).map((m) => <option key={m.id} value={m.id}>{m.label || m.id}</option>)}
              {defaultModel && !(harness?.model_choices ?? []).some((m) => m.id === defaultModel) && <option value={defaultModel}>{defaultModel}</option>}
            </select>
          ) : (
            <input type="text" className="mono" value={defaultModel} onChange={(e) => setDefaultModel(e.target.value)} placeholder="The harness’s own default" />
          )}
          <span className="hint">Used when someone starts work without choosing a model.</span>
        </label>
        <label className="field grow">
          Model environment variable
          <input type="text" className="mono" value={modelEnv} onChange={(e) => setModelEnv(e.target.value)} placeholder="none" />
          <span className="hint">Optional. Also set to the chosen model, for agents that read it from the environment.</span>
        </label>
      </div>
      <label className="field">
        Models people may choose
        <textarea className="code" rows={3} style={{ minHeight: 0 }} value={models} onChange={(e) => setModels(e.target.value)} placeholder="Leave empty to allow every model the harness offers" />
        <span className="hint">
          One model id per line, optionally as id=Label. Listing any restricts the choice to these.
          {discovered.length > 0 && <> This harness offers: {discovered.map((m) => m.id).join(", ")}.</>}
        </span>
      </label>
      <label className="field">Credentials it accepts<textarea className="code" rows={3} style={{ minHeight: 0 }} value={credentials} onChange={(e) => setCredentials(e.target.value)} placeholder={"MY_API_KEY=My provider API key"} /><span className="hint">One per line as ENV_NAME=Label. Add [oauth] at the end for a personal subscription token.</span></label>
      <label className="field">Hosts it may reach<textarea className="code" rows={3} style={{ minHeight: 0 }} value={egress} onChange={(e) => setEgress(e.target.value)} placeholder={"api.example.com\n*.example.com"} /><span className="hint">One per line. Enforced when the egress proxy is enabled.</span></label>
      <ErrorNote error={action.error} />
      <div className="modal-foot">
        {harness && !harness.builtin && <button className="btn btn-danger" style={{ marginRight: "auto" }} disabled={action.busy} onClick={() => void remove()}>Delete</button>}
        <button className="btn" onClick={onClose}>Cancel</button>
        <button className="btn btn-primary" disabled={action.busy} onClick={() => void save()}>Save</button>
      </div>
    </Modal>
  );
}

// ------------------------------------------------------------------- general

function General() {
  const { data, error, reload } = useLoad<any>(() => api.get("/settings"), []);
  const harnesses = useLoad<{ harnesses: Harness[] }>(() => api.get("/harnesses"), []);
  const [form, setForm] = useState<{ team_name: string; default_harness: string; egress: string } | null>(null);
  const [saved, setSaved] = useState(false);
  const action = useAction();
  if (data && !form) setForm({ team_name: data.team_name ?? "", default_harness: data.default_harness, egress: (data.egress_allow as string[]).join("\n") });
  if (!data || !form) return <ErrorNote error={error} />;
  const save = () =>
    action.run(async () => {
      await api.put("/settings", { team_name: form.team_name, default_harness: form.default_harness, egress_allow: form.egress.split("\n").map((l) => l.trim()).filter(Boolean) });
      setSaved(true);
      reload();
    });
  return (
    <div className="section stack" style={{ maxWidth: 560 }}>
      <label className="field">Team name<input type="text" value={form.team_name} onChange={(e) => (setSaved(false), setForm({ ...form, team_name: e.target.value }))} /></label>
      <label className="field">
        Default harness
        <select value={form.default_harness} onChange={(e) => (setSaved(false), setForm({ ...form, default_harness: e.target.value }))}>
          {harnesses.data?.harnesses.map((h) => <option key={h.id} value={h.name}>{h.title}</option>)}
        </select>
      </label>
      <label className="field">
        Extra hosts every sandbox may reach
        <textarea className="code" rows={4} value={form.egress} onChange={(e) => (setSaved(false), setForm({ ...form, egress: e.target.value }))} placeholder={"internal-registry.example.com"} />
        <span className="hint">
          {data.egress_enforced
            ? <>Sandboxes reach the internet only through Noemune’s allowlist. Always allowed: {data.egress_allow_builtin.join(", ")}, plus each harness’s own hosts.</>
            : <>The egress proxy is not enabled on this installation, so sandbox network access is not restricted by Noemune. This list takes effect once it is.</>}
        </span>
      </label>
      <ErrorNote error={action.error} />
      {saved && <div className="ok-note">Saved.</div>}
      <div><button className="btn btn-primary" disabled={action.busy} onClick={() => void save()}>Save</button></div>
      <p className="small muted">
        Idle sandboxes are torn down after {Math.round(data.idle_timeout_secs / 60)} minutes and at most {data.max_sessions} run at once. Both are set when the server starts.
      </p>
    </div>
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
    <div className="section">
      <div className="spread" style={{ marginBottom: 12 }}>
        <p className="lead" style={{ margin: 0 }}>Every attempt by an agent to act outside its sandbox: tool calls, git operations and blocked network requests.</p>
        <label className="check"><input type="checkbox" checked={deniedOnly} onChange={(e) => setDeniedOnly(e.target.checked)} /> Blocked only</label>
      </div>
      <ErrorNote error={error} />
      {data && data.audit.length === 0 && <Empty title="Nothing recorded yet" />}
      {data && data.audit.length > 0 && (
        <div className="card" style={{ overflowX: "auto" }}>
          <table>
            <thead><tr><th>When</th><th>Agent</th><th>On behalf of</th><th>Action</th><th>Target</th><th>Outcome</th></tr></thead>
            <tbody>
              {data.audit.map((row) => (
                <tr key={row.id}>
                  <td className="small muted" style={{ whiteSpace: "nowrap" }}>{timeAgo(row.ts)}</td>
                  <td>{row.session_id ? <A href={`/s/${row.session_id}`}>{row.session_title ?? "session"}</A> : "—"}</td>
                  <td>{row.user_id ? users[row.user_id]?.name ?? "—" : "—"}</td>
                  <td className="mono">{row.action}</td>
                  <td className="mono">{row.target}</td>
                  <td title={row.detail}>
                    {row.allowed ? (row.detail ? <span className="chip">Failed</span> : <span className="chip chip-green">Allowed</span>) : <span className="chip chip-red">Blocked</span>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

// ------------------------------------------------------------------- account

interface ApiKey {
  id: string;
  name: string;
  created_at: string;
  last_used_at: string | null;
}

function Account() {
  const me = useStore((s) => s.me)!;
  const keys = useLoad<{ api_keys: ApiKey[] }>(() => api.get("/me/api-keys"), []);
  const [name, setName] = useState(me.name);
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [keyName, setKeyName] = useState("");
  const [newKey, setNewKey] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const action = useAction();

  const saveName = () => action.run(async () => (await api.patch("/me", { name }), await refreshMe(), setSaved("Name updated.")));
  const savePassword = () => action.run(async () => (await api.patch("/me", { current_password: current, new_password: next }), setCurrent(""), setNext(""), setSaved("Password changed.")));
  const createKey = () => action.run(async () => (setNewKey((await api.post("/me/api-keys", { name: keyName })).token), setKeyName(""), keys.reload()));

  return (
    <div className="section stack" style={{ maxWidth: 620, gap: 26 }}>
      <ErrorNote error={action.error} />
      {saved && <div className="ok-note">{saved}</div>}
      <div className="stack">
        <h2>Profile</h2>
        <div className="row"><input type="text" value={name} onChange={(e) => setName(e.target.value)} /><button className="btn" disabled={action.busy || !name.trim() || name === me.name} onClick={() => void saveName()}>Save</button></div>
        <div className="small muted">{me.email}</div>
      </div>
      <div className="stack">
        <h2>Password</h2>
        <label className="field">Current password<input type="password" autoComplete="current-password" value={current} onChange={(e) => setCurrent(e.target.value)} /></label>
        <label className="field">New password<input type="password" autoComplete="new-password" value={next} onChange={(e) => setNext(e.target.value)} /><span className="hint">At least 10 characters.</span></label>
        <div><button className="btn" disabled={action.busy || !current || next.length < 10} onClick={() => void savePassword()}>Change password</button></div>
      </div>
      <div className="stack">
        <div>
          <h2>API keys</h2>
          <p className="muted" style={{ margin: "2px 0 0" }}>For the <code>noemune</code> command-line tool and your own scripts. A key acts as you.</p>
        </div>
        {newKey && (
          <div className="ok-note">
            Copy this key now; it will not be shown again.
            <input type="text" readOnly value={newKey} style={{ marginTop: 6 }} onFocus={(e) => e.target.select()} />
          </div>
        )}
        {keys.data && keys.data.api_keys.length > 0 && (
          <div className="card">
            <table>
              <tbody>
                {keys.data.api_keys.map((key) => (
                  <tr key={key.id}>
                    <td><strong>{key.name}</strong></td>
                    <td className="small muted">created {timeAgo(key.created_at)} · {key.last_used_at ? `last used ${timeAgo(key.last_used_at)}` : "never used"}</td>
                    <td style={{ textAlign: "right" }}><button className="btn btn-sm btn-ghost" onClick={() => void action.run(async () => (await api.del(`/me/api-keys/${key.id}`), keys.reload()))}>Revoke</button></td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        <div className="row"><input type="text" placeholder="Key name, e.g. laptop" value={keyName} onChange={(e) => setKeyName(e.target.value)} /><button className="btn" disabled={action.busy || !keyName.trim()} onClick={() => void createKey()}>Create key</button></div>
      </div>
    </div>
  );
}
