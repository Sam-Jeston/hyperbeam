// MCP connections: first-class services (Linear, Notion, PostHog, Sentry) and any
// other MCP server a team wants its agents to reach, through the gateway.

import { Plus, RefreshCw, Search, ShieldCheck, Wrench } from "lucide-react";
import { type ReactNode, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { Button } from "../components/ui/button";
import { SwitchRow } from "../components/ui/controls";
import { confirm, Modal } from "../components/ui/dialog";
import { Field, Input } from "../components/ui/input";
import { Select } from "../components/ui/select";
import { Badge, Card, EmptyState, Notice, Skeleton } from "../components/ui/surface";
import { api, type Connection, type ConnectionTool, type Preset, type ToolLevel } from "../lib/api";
import { reloadGrants } from "../lib/store";
import { ErrorNote, SectionTitle, Time, useAction, useLoad } from "../lib/ui";
import { cn } from "../lib/utils";

interface Listing {
  presets: Preset[];
  connections: Connection[];
  /** Where OAuth sign-ins come back to; registered with the vendor when an admin creates a client by hand. */
  oauth_redirect_uri: string;
}

/** A save either takes effect, or needs the admin to sign in with the vendor first. */
type Saved = { connection: Connection; authorize_url?: undefined } | { authorize_url: string; connection?: undefined };

/** Send the browser to the vendor to sign in. It comes back to this screen. */
const signIn = (url: string) => new Promise<never>(() => window.location.assign(url));

/** One card per first-class service, connected or not, then custom connections. */
export function Connections({ isAdmin }: { isAdmin: boolean }) {
  const { data, error, reload } = useLoad<Listing>(() => api.get("/connections"), []);
  const [editing, setEditing] = useState<{ preset: Preset | null; connection: Connection | null } | null>(null);
  const [reviewing, setReviewing] = useState<string | null>(null);
  const changed = () => (reload(), void reloadGrants());

  // Back from an OAuth sign-in: say how it went, then tidy the address bar.
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const [connected, failed] = [params.get("connected"), params.get("oauth_error")];
    if (!connected && !failed) return;
    window.history.replaceState(null, "", window.location.pathname);
    if (failed) toast.error(failed, { duration: 12000 });
    else if (connected) {
      toast.success("Signed in");
      void reloadGrants();
      // Fresh from the server: show what it offers straight away.
      setReviewing(connected);
    }
  }, []);
  const custom = data?.connections.filter((c) => !c.preset) ?? [];
  const reviewed = data?.connections.find((c) => c.name === reviewing) ?? null;

  return (
    <section className="mt-10">
      <SectionTitle
        description="MCP servers agents reach through Hyperbeam. Each tool is a read tool, a write tool, or hidden; people need that connection’s read or write permission to use it."
        action={isAdmin && <Button onClick={() => setEditing({ preset: null, connection: null })}><Plus /> Add custom connection</Button>}
      >
        Connected services
      </SectionTitle>
      <ErrorNote error={error} />
      {!data && !error && <div className="grid gap-3">{[0, 1, 2].map((i) => <Skeleton key={i} className="h-24 rounded-lg" />)}</div>}
      <div className="grid gap-3">
        {data?.presets.map((preset) => {
          const connection = data.connections.find((c) => c.preset === preset.id) ?? null;
          return (
            <ConnectionCard
              key={preset.id}
              title={preset.title}
              blurb={preset.blurb}
              connection={connection}
              isAdmin={isAdmin}
              onEdit={() => setEditing({ preset, connection })}
              onReview={() => setReviewing(preset.id)}
            />
          );
        })}
        {custom.map((connection) => (
          <ConnectionCard
            key={connection.name}
            title={connection.title}
            blurb={<span className="font-mono text-[12.5px]">{connection.url}</span>}
            connection={connection}
            isAdmin={isAdmin}
            onEdit={() => setEditing({ preset: null, connection })}
            onReview={() => setReviewing(connection.name)}
          />
        ))}
        {data && custom.length === 0 && isAdmin && (
          <EmptyState compact icon={<Wrench />} title="Another MCP server?">
            Add any server that speaks MCP over streamable HTTP, such as an internal tool or a vendor without first-class support. Its tools stay hidden until you say what each is for.
          </EmptyState>
        )}
      </div>
      {editing && (
        <ConnectionDialog
          preset={editing.preset}
          connection={editing.connection}
          redirectUri={data?.oauth_redirect_uri ?? ""}
          onClose={() => setEditing(null)}
          onSaved={(saved) => {
            setEditing(null);
            changed();
            // Fresh from the server: show what it offers straight away.
            if (saved && (!editing.connection || saved.awaiting_review > 0)) setReviewing(saved.name);
          }}
        />
      )}
      {reviewed && <ToolsDialog connection={reviewed} isAdmin={isAdmin} onClose={() => setReviewing(null)} onChanged={changed} />}
    </section>
  );
}

function ConnectionCard(props: {
  title: string;
  blurb: ReactNode;
  connection: Connection | null;
  isAdmin: boolean;
  onEdit: () => void;
  onReview: () => void;
}) {
  const { connection } = props;
  const usable = connection?.tools.filter((t) => t.level !== "hidden").length ?? 0;
  return (
    <Card className="grid gap-3 p-4">
      <div className="flex flex-wrap items-start gap-4">
        <div className="grid size-10 shrink-0 place-items-center rounded-lg bg-surface-2 font-display text-[17px] font-bold text-foreground">{props.title.charAt(0).toUpperCase()}</div>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="font-display text-[15.5px] font-semibold">{props.title}</h3>
            {!connection ? (
              <Badge>Not connected</Badge>
            ) : connection.tools_error ? (
              <Badge tone="fail">Needs attention</Badge>
            ) : (
              <Badge tone="run"><span className="size-1.5 rounded-full bg-run" />Connected</Badge>
            )}
            {connection && !connection.tools_error && <Badge tone="outline">{usable} of {connection.tools.length} tools in use</Badge>}
            {connection && connection.awaiting_review > 0 && <Badge tone="wait">{connection.awaiting_review} awaiting review</Badge>}
          </div>
          <p className="mt-0.5 min-w-0 truncate text-[13px] text-muted">{props.blurb}</p>
        </div>
        <div className="flex gap-2">
          {connection && <Button onClick={props.onReview}><ShieldCheck /> Tools</Button>}
          {props.isAdmin && <Button variant={connection ? "secondary" : "primary"} onClick={props.onEdit}>{connection ? "Change" : "Connect"}</Button>}
        </div>
      </div>
      {connection?.tools_error && <Notice tone="error">{connection.tools_error}</Notice>}
    </Card>
  );
}

const AUTH_OPTIONS = [
  { value: "bearer", label: "Bearer token", description: "Sent as Authorization: Bearer …" },
  { value: "header", label: "Custom header", description: "Sent as-is in a header you name, e.g. X-Api-Key" },
  { value: "oauth", label: "OAuth sign-in", description: "You sign in with the service; Hyperbeam keeps the token fresh" },
  { value: "none", label: "No credential", description: "For servers inside your network that need none" },
];

function authKind(connection: Connection | null): string {
  if (connection?.auth.oauth) return "oauth";
  if (!connection?.auth.header) return connection ? "none" : "bearer";
  return connection.auth.header.toLowerCase() === "authorization" && connection.auth.prefix === "Bearer " ? "bearer" : "header";
}

function ConnectionDialog(props: { preset: Preset | null; connection: Connection | null; redirectUri: string; onClose: () => void; onSaved: (saved: Connection | null) => void }) {
  const { preset, connection } = props;
  const [name, setName] = useState(connection?.name ?? "");
  const [title, setTitle] = useState(connection?.title ?? "");
  const [url, setUrl] = useState(connection?.url ?? "");
  const [auth, setAuth] = useState(authKind(connection));
  const [header, setHeader] = useState(authKind(connection) === "header" ? connection?.auth.header ?? "" : "");
  const [options, setOptions] = useState<Record<string, string>>(() => {
    const initial: Record<string, string> = {};
    for (const option of preset?.options ?? []) initial[option.key] = connection?.options[option.key] ?? option.default ?? "";
    return initial;
  });
  const [secret, setSecret] = useState("");
  const [clientId, setClientId] = useState(connection?.auth.oauth?.client_id ?? "");
  const [clientSecret, setClientSecret] = useState("");
  const [scope, setScope] = useState(connection?.auth.oauth?.scope ?? "");
  const action = useAction();
  const label = preset?.title ?? (connection?.title || "custom connection");
  const oauth = preset ? preset.auth === "oauth" : auth === "oauth";
  const signedIn = Boolean(connection?.auth.oauth);
  const needsSecret = !oauth && (preset ? true : auth !== "none");
  const ready = connection ? true : (preset || (name.trim() && url.trim())) && (!needsSecret || secret.trim());

  const save = () =>
    action.run(async () => {
      const body = preset
        ? { preset: preset.id, options, secret: secret || undefined }
        : {
            name: name.trim(),
            title: title.trim() || undefined,
            url: url.trim(),
            auth: { kind: auth, header: header.trim() || undefined, client_id: clientId.trim() || undefined, client_secret: clientSecret || undefined, scope: scope.trim() || undefined },
            secret: secret || undefined,
          };
      const result: Saved = connection ? await api.patch(`/connections/${connection.name}`, body) : await api.post("/connections", body);
      if (result.authorize_url) await signIn(result.authorize_url);
      else {
        toast.success(`${label} ${connection ? "updated" : "connected"}`);
        props.onSaved(result.connection ?? null);
      }
    });
  const signInAgain = () => action.run(async () => signIn((await api.post(`/connections/${connection!.name}/authorize`)).authorize_url));

  const disconnect = async () => {
    const ok = await confirm({
      title: `Disconnect ${label}?`,
      description: `Agents lose access to ${label} straight away, including ones that are running. The stored credential is deleted, and everyone’s ${label} permissions are removed.`,
      confirmLabel: "Disconnect",
      destructive: true,
    });
    if (ok) void action.run(async () => (await api.del(`/connections/${connection!.name}`), props.onSaved(null)), { success: `${label} disconnected` });
  };

  return (
    <Modal
      title={`${connection ? "Change" : "Connect"} ${label}`}
      onClose={props.onClose}
      modalOnly
      footer={
        <>
          {connection && <Button variant="danger-ghost" className="mr-auto" disabled={action.busy} onClick={() => void disconnect()}>Disconnect</Button>}
          <Button onClick={props.onClose}>Cancel</Button>
          {signedIn && <Button disabled={action.busy} onClick={() => void signInAgain()}>Sign in again</Button>}
          <Button variant="primary" disabled={!ready} loading={action.busy} onClick={() => void save()}>{connection ? "Save" : oauth ? `Sign in with ${label}` : "Connect"}</Button>
        </>
      }
    >
      {preset ? (
        <>
          <p className="text-[13px] text-muted">{preset.secret_help}</p>
          {preset.options.map((option) => (
            <Field key={option.key} label={option.label} hint={option.hint} optional={!option.choices}>
              {(id) =>
                option.choices ? (
                  <Select id={id} value={options[option.key] ?? ""} onValueChange={(v) => setOptions({ ...options, [option.key]: v })} options={option.choices} />
                ) : (
                  <Input id={id} value={options[option.key] ?? ""} placeholder={option.placeholder} onChange={(e) => setOptions({ ...options, [option.key]: e.target.value })} />
                )
              }
            </Field>
          ))}
          <Notice tone="info">
            {preset.classified_by === "vendor"
              ? `Hyperbeam sorts ${preset.title}’s tools into read and write using ${preset.title}’s own read-only server, and sends read calls through it.`
              : `Hyperbeam sorts ${preset.title}’s tools into read and write by name. Tools it cannot place stay hidden until you review them.`}
          </Notice>
        </>
      ) : (
        <>
          {!connection && (
            <Field label="Name" hint="Lowercase letters, digits and dashes. Agents see its tools as name_tool, and permissions are name:read and name:write. It cannot be changed later.">
              {(id) => <Input id={id} mono autoFocus value={name} onChange={(e) => setName(e.target.value.toLowerCase())} placeholder="acme-tools" />}
            </Field>
          )}
          <Field label="Display name" optional>{(id) => <Input id={id} value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Acme internal tools" />}</Field>
          <Field label="Server URL" hint="An MCP server that speaks streamable HTTP. Hyperbeam calls it from the control plane; agents never reach it directly.">
            {(id) => <Input id={id} mono value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://mcp.example.com/mcp" />}
          </Field>
          <Field label="Credential">{(id) => <Select id={id} value={auth} onValueChange={setAuth} options={AUTH_OPTIONS} />}</Field>
          {auth === "header" && <Field label="Header name">{(id) => <Input id={id} mono value={header} onChange={(e) => setHeader(e.target.value)} placeholder="X-Api-Key" />}</Field>}
          {auth === "oauth" && (
            <>
              <p className="text-[13px] text-muted">
                You will be sent to the service to sign in, and agents act as the account you sign in with. Most MCP servers let Hyperbeam register itself, so the fields below can stay empty. If this one does not, create an OAuth client with it using the redirect URL <code className="font-mono text-[12.5px] break-all">{props.redirectUri}</code> and enter its details.
              </p>
              <Field label="Client ID" optional>{(id) => <Input id={id} mono autoComplete="off" value={clientId} onChange={(e) => setClientId(e.target.value)} />}</Field>
              <Field label="Client secret" optional hint={signedIn ? "Leave blank to keep the current one. Stored encrypted and never shown again." : "Only if the client has one. Stored encrypted and never shown again."}>
                {(id) => <Input id={id} type="password" mono autoComplete="off" value={clientSecret} onChange={(e) => setClientSecret(e.target.value)} />}
              </Field>
              <Field label="Scopes" optional hint="Space-separated. Leave blank to ask for what the server suggests.">
                {(id) => <Input id={id} mono value={scope} onChange={(e) => setScope(e.target.value)} />}
              </Field>
            </>
          )}
        </>
      )}
      {needsSecret && (
        <Field
          label={preset?.secret_label ?? (auth === "bearer" ? "Token" : "Header value")}
          hint={connection ? "Leave blank to keep the current one. Stored encrypted and never shown again." : "Stored encrypted and never shown again."}
        >
          {(id) => <Input id={id} type="password" mono autoComplete="off" value={secret} onChange={(e) => setSecret(e.target.value)} placeholder={connection ? "••••••••••••" : undefined} />}
        </Field>
      )}
      <ErrorNote error={action.error} />
    </Modal>
  );
}

const STATUS: Record<ConnectionTool["status"], { label: string; tone: "neutral" | "outline" | "wait" | "fail" }> = {
  approved: { label: "Set by an admin", tone: "outline" },
  automatic: { label: "Automatic", tone: "neutral" },
  pending: { label: "Awaiting review", tone: "wait" },
  changed: { label: "Changed since review", tone: "fail" },
};

/** What each of a connection's tools is for, and who may use it. */
function ToolsDialog(props: { connection: Connection; isAdmin: boolean; onClose: () => void; onChanged: () => void }) {
  const { connection, isAdmin } = props;
  // Unsaved decisions; `null` means "back to automatic".
  const [draft, setDraft] = useState<Record<string, ToolLevel | null>>({});
  const [query, setQuery] = useState("");
  const [onlyWaiting, setOnlyWaiting] = useState(connection.awaiting_review > 0);
  const action = useAction();
  const isPreset = Boolean(connection.preset);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return connection.tools.filter(
      (t) =>
        (!onlyWaiting || t.status === "pending" || t.status === "changed" || t.name in draft) &&
        (!q || t.name.toLowerCase().includes(q) || t.description.toLowerCase().includes(q)),
    );
  }, [connection, query, onlyWaiting, draft]);

  const choice = (tool: ConnectionTool): string => {
    if (tool.name in draft) return draft[tool.name] ?? "auto";
    if (tool.status === "automatic") return "auto";
    if (tool.status === "approved") return tool.level;
    return "";
  };
  const options = (tool: ConnectionTool) => [
    ...(isPreset && tool.suggested ? [{ value: "auto", label: `Automatic (${tool.suggested})` }] : []),
    { value: "read", label: "Read", description: `Needs ${connection.name}:read` },
    { value: "write", label: "Write", description: `Needs ${connection.name}:write` },
    { value: "hidden", label: "Hidden", description: "No agent sees it" },
  ];
  const pick = (tool: ConnectionTool, value: string) => setDraft({ ...draft, [tool.name]: value === "auto" ? null : (value as ToolLevel) });
  const pending = shown.filter((t) => (t.status === "pending" || t.status === "changed") && !(t.name in draft));
  const markAll = (level: ToolLevel) => setDraft({ ...draft, ...Object.fromEntries(pending.map((t) => [t.name, level])) });

  const save = () =>
    action.run(async () => {
      await api.put(`/connections/${connection.name}/approvals`, { approvals: draft });
      setDraft({});
      props.onChanged();
    }, { success: "Tools updated" });
  const refresh = () => action.run(async () => (await api.post(`/connections/${connection.name}/refresh`), props.onChanged()), { success: "Tools re-listed" });
  const dirty = Object.keys(draft).length;

  return (
    <Modal
      size="lg"
      modalOnly={dirty > 0}
      title={`${connection.title} tools`}
      description={
        <>
          {connection.tools.length} tools{connection.tools_refreshed_at && <>, listed <Time iso={connection.tools_refreshed_at} /></>}. Read tools need <code className="font-mono">{connection.name}:read</code>; write tools need <code className="font-mono">{connection.name}:write</code>.
          {!isPreset && " If the server changes a tool after you review it, the tool is hidden again until you look at it."}
        </>
      }
      onClose={props.onClose}
      footer={
        <>
          {isAdmin && <Button variant="ghost" className="mr-auto" disabled={action.busy} onClick={() => void refresh()}><RefreshCw /> Re-list tools</Button>}
          <Button onClick={props.onClose}>{dirty ? "Discard" : "Close"}</Button>
          {isAdmin && <Button variant="primary" disabled={!dirty} loading={action.busy} onClick={() => void save()}>Save {dirty > 0 && `${dirty} change${dirty > 1 ? "s" : ""}`}</Button>}
        </>
      }
    >
      {connection.tools_error && <Notice tone="error">{connection.tools_error}</Notice>}
      <div className="flex flex-wrap items-center gap-3">
        <div className="relative min-w-[220px] flex-1">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-faint" />
          <Input className="pl-8" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Find a tool" aria-label="Find a tool" />
        </div>
        <div className="min-w-[200px]"><SwitchRow checked={onlyWaiting} onCheckedChange={setOnlyWaiting} label="Awaiting review only" /></div>
      </div>
      {isAdmin && pending.length > 1 && (
        <div className="flex flex-wrap items-center gap-2 text-[13px] text-muted">
          {pending.length} shown awaiting review:
          <Button size="sm" variant="ghost" onClick={() => markAll("read")}>Mark all read</Button>
          <Button size="sm" variant="ghost" onClick={() => markAll("write")}>Mark all write</Button>
          <Button size="sm" variant="ghost" onClick={() => markAll("hidden")}>Hide all</Button>
        </div>
      )}
      <div className="grid divide-y divide-border rounded-lg border border-border">
        {shown.length === 0 && <p className="px-3.5 py-6 text-center text-[13px] text-muted">{onlyWaiting ? "Nothing is awaiting review." : "No tools match."}</p>}
        {shown.map((tool) => {
          const status = STATUS[tool.status];
          const writes = tool.annotations?.readOnlyHint === false || tool.annotations?.destructiveHint === true;
          return (
            <div key={tool.name} className={cn("flex flex-wrap items-start gap-3 px-3.5 py-2.5", tool.name in draft && "bg-accent/30")}>
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-1.5">
                  <span className="font-mono text-[12.5px] font-medium">{tool.name}</span>
                  <Badge tone={status.tone}>{status.label}</Badge>
                  {tool.annotations?.readOnlyHint === true && <Badge tone="outline">Server says read-only</Badge>}
                  {writes && <Badge tone="outline">Server says it changes things</Badge>}
                </div>
                <p className="mt-0.5 line-clamp-2 text-[12.5px] text-muted" title={tool.description}>{tool.description || "No description."}</p>
              </div>
              {isAdmin ? (
                <Select
                  size="sm"
                  className="w-44"
                  aria-label={`What ${tool.name} is for`}
                  value={choice(tool)}
                  placeholder="Choose…"
                  onValueChange={(v) => pick(tool, v)}
                  options={options(tool)}
                />
              ) : (
                <span className="text-[12.5px] capitalize text-muted">{tool.level}</span>
              )}
            </div>
          );
        })}
      </div>
      <ErrorNote error={action.error} />
    </Modal>
  );
}
