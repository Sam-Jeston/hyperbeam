import { MailPlus, UserMinus, UserPlus, Undo2 } from "lucide-react";
import { useState } from "react";
import { Button } from "../components/ui/button";
import { Checkbox, CheckRow } from "../components/ui/controls";
import { confirm, Modal } from "../components/ui/dialog";
import { Field, Input } from "../components/ui/input";
import { Select } from "../components/ui/select";
import { Badge, Card, Notice } from "../components/ui/surface";
import { api, type GrantInfo, type Repo, type Role, type User } from "../lib/api";
import { refreshMe, reloadUsers, useStore } from "../lib/store";
import { Avatar, CopyField, systemName, ErrorNote, Page, PageHeader, SectionTitle, useAction, useDocumentTitle, useLoad } from "../lib/ui";
import { cn } from "../lib/utils";

interface Invite {
  id: string;
  email: string;
  role: Role;
  grants: string[];
  expires_at: string;
}

/** Ticking a stronger permission ticks what it depends on, as the server does. */
function withImplied(grants: string[]): string[] {
  const set = new Set(grants);
  for (const g of grants) {
    const [system, level] = g.split(":");
    if (level !== "read") set.add(`${system}:read`);
  }
  return [...set];
}

function toggleGrant(grants: string[], id: string): string[] {
  if (!grants.includes(id)) return withImplied([...grants, id]);
  const [system, level] = id.split(":");
  // Removing read removes everything that depends on it.
  return level === "read" ? grants.filter((g) => !g.startsWith(`${system}:`)) : grants.filter((g) => g !== id);
}

const ROLE_HELP: Record<Role, string> = {
  member: "Starts agents and builds skills and workflows",
  admin: "Also manages people, integrations and harnesses",
  owner: "Everything, including other admins",
};

export function TeamPage() {
  useDocumentTitle("People & permissions");
  const me = useStore((s) => s.me)!;
  const meta = useStore((s) => s.meta)!;
  const users = useStore((s) => s.users);
  const isAdmin = me.role !== "member";
  const invites = useLoad<{ invites: Invite[] }>(() => (isAdmin ? api.get("/invites") : Promise.resolve({ invites: [] })), [isAdmin]);
  const repos = useLoad<{ repos: Repo[] }>(() => api.get("/repos"), []);
  const [inviting, setInviting] = useState(false);
  const [scoping, setScoping] = useState<User | null>(null);
  const action = useAction();

  const people = Object.values(users).sort((a, b) => a.created_at.localeCompare(b.created_at));
  const systems = [...new Set(meta.grants.map((g) => g.system))];

  const update = (user: User, patch: Record<string, unknown>, success?: string) =>
    action.run(async () => {
      await api.patch(`/users/${user.id}`, patch);
      await reloadUsers();
      if (user.id === me.id) await refreshMe();
    }, { success, toastError: true });

  const remove = async (user: User) => {
    const ok = await confirm({
      title: `Remove ${user.name}?`,
      description: "They are signed out and their running agents stop. You can restore them later; their history stays.",
      confirmLabel: "Remove",
      destructive: true,
    });
    if (ok) void update(user, { disabled: true }, `${user.name} removed`);
  };

  return (
    <Page width="wide">
      <PageHeader
        title="People & permissions"
        description={<>An agent can do only what the person who started it is allowed to. Set that here, per person.{!isAdmin && " Only admins can change these."}</>}
        actions={isAdmin && <Button variant="primary" onClick={() => setInviting(true)}><UserPlus /> Invite someone</Button>}
      />

      <Card className="overflow-x-auto">
        <table className="w-full min-w-[860px] border-collapse text-[13.5px]">
          <thead>
            <tr className="text-left">
              <th rowSpan={2} className="border-b border-border px-4 py-2.5 align-bottom text-[12px] font-medium text-muted">Person</th>
              <th rowSpan={2} className="border-b border-border px-3 py-2.5 align-bottom text-[12px] font-medium text-muted">Role</th>
              {systems.map((system) => (
                <th key={system} colSpan={meta.grants.filter((g) => g.system === system).length} className="border-l border-border px-2 pt-2.5 pb-1 text-center text-[12px] font-semibold">
                  {systemName(system)}
                </th>
              ))}
              <th rowSpan={2} className="border-b border-l border-border px-3 py-2.5 align-bottom text-[12px] font-medium text-muted">Repositories</th>
              {isAdmin && <th rowSpan={2} className="border-b border-border" />}
            </tr>
            <tr>
              {meta.grants.map((g, i) => (
                <th
                  key={g.id}
                  title={g.label}
                  className={cn("border-b border-border px-1.5 pb-2 text-center text-[11.5px] font-medium leading-tight text-muted", (i === 0 || meta.grants[i - 1].system !== g.system) && "border-l")}
                >
                  {shortLabel(g)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody className="divide-y divide-border">
            {people.map((user) => {
              const locked = !isAdmin || (user.role === "owner" && me.role !== "owner");
              return (
                <tr key={user.id} className={cn("transition-colors hover:bg-surface-2/40", user.disabled && "opacity-50")}>
                  <td className="px-4 py-2.5">
                    <div className="flex items-center gap-2.5">
                      <Avatar user={user} size={28} className="ring-0" />
                      <div className="min-w-0">
                        <div className="flex items-center gap-1.5 font-medium">{user.name}{user.id === me.id && <Badge>You</Badge>}</div>
                        <div className="truncate text-[12.5px] text-muted">{user.email}</div>
                      </div>
                    </div>
                  </td>
                  <td className="px-3 py-2.5">
                    {locked || user.id === me.id || user.disabled ? (
                      <span className="capitalize text-muted">{user.disabled ? "Removed" : user.role}</span>
                    ) : (
                      <Select
                        size="sm"
                        className="w-28"
                        aria-label={`${user.name}'s role`}
                        value={user.role}
                        onValueChange={(v) => void update(user, { role: v }, `${user.name} is now ${v === "admin" ? "an admin" : `a ${v}`}`)}
                        options={(["member", "admin", ...(me.role === "owner" ? ["owner"] : [])] as Role[]).map((r) => ({ value: r, label: <span className="capitalize">{r}</span>, description: ROLE_HELP[r] }))}
                      />
                    )}
                  </td>
                  {meta.grants.map((g, i) => (
                    <td key={g.id} className={cn("px-1.5 py-2.5 text-center", (i === 0 || meta.grants[i - 1].system !== g.system) && "border-l border-border")}>
                      <span className="inline-flex">
                        <Checkbox
                          aria-label={`${user.name}: ${g.label}`}
                          checked={user.grants.includes(g.id)}
                          disabled={locked || user.disabled || action.busy}
                          onCheckedChange={() => void update(user, { grants: toggleGrant(user.grants, g.id) }, `Permissions updated for ${user.name}`)}
                        />
                      </span>
                    </td>
                  ))}
                  <td className="border-l border-border px-3 py-2.5">
                    <Button variant="ghost" size="sm" disabled={locked || user.disabled} onClick={() => setScoping(user)}>
                      {user.repo_scope === null ? "All" : `${user.repo_scope.length} selected`}
                    </Button>
                  </td>
                  {isAdmin && (
                    <td className="px-3 py-2.5 text-right">
                      {user.id !== me.id && !locked && (
                        user.disabled ? (
                          <Button variant="ghost" size="sm" onClick={() => void update(user, { disabled: false }, `${user.name} restored`)}><Undo2 /> Restore</Button>
                        ) : (
                          <Button variant="danger-ghost" size="sm" onClick={() => void remove(user)}><UserMinus /> Remove</Button>
                        )
                      )}
                    </td>
                  )}
                </tr>
              );
            })}
          </tbody>
        </table>
      </Card>
      <p className="mt-3 max-w-[90ch] text-[12.5px] text-muted">
        Changes apply immediately, including to agents that are already running. Removing someone signs them out and stops their agents.
        Admins manage people, integrations and harnesses; an admin’s own agents are still limited to the permissions ticked here.
      </p>

      {isAdmin && invites.data && invites.data.invites.length > 0 && (
        <section className="mt-10 max-w-[760px]">
          <SectionTitle>Pending invites</SectionTitle>
          <Card className="divide-y divide-border">
            {invites.data.invites.map((invite) => (
              <div key={invite.id} className="flex flex-wrap items-center gap-3 px-4 py-2.5">
                <MailPlus className="size-4 text-faint" />
                <span className="font-medium">{invite.email}</span>
                <Badge className="capitalize">{invite.role}</Badge>
                <span className="text-[12.5px] text-muted">Expires {new Date(invite.expires_at).toLocaleDateString(undefined, { month: "short", day: "numeric" })}</span>
                <Button
                  variant="ghost"
                  size="sm"
                  className="ml-auto"
                  onClick={async () => {
                    if (await confirm({ title: `Revoke the invite for ${invite.email}?`, description: "The link stops working. You can invite them again later.", confirmLabel: "Revoke", destructive: true })) {
                      void action.run(async () => (await api.del(`/invites/${invite.id}`), invites.reload()), { success: "Invite revoked", toastError: true });
                    }
                  }}
                >
                  Revoke
                </Button>
              </div>
            ))}
          </Card>
        </section>
      )}

      {inviting && <InviteDialog grants={meta.grants} onClose={() => (setInviting(false), invites.reload())} />}
      {scoping && (
        <ScopeDialog
          user={scoping}
          repos={repos.data?.repos ?? []}
          onClose={() => setScoping(null)}
          onSave={async (scope) => {
            await update(scoping, { repo_scope: scope }, `Repository access updated for ${scoping.name}`);
            setScoping(null);
          }}
        />
      )}
    </Page>
  );
}

function shortLabel(g: GrantInfo): string {
  const level = g.id.split(":")[1];
  return ({ read: "Read", write: "Write", open_pr: "Open PRs", merge_pr: "Merge PRs" } as Record<string, string>)[level] ?? level;
}

function InviteDialog({ grants, onClose }: { grants: GrantInfo[]; onClose: () => void }) {
  const [email, setEmail] = useState("");
  const [role, setRole] = useState<Role>("member");
  const [picked, setPicked] = useState<string[]>(["github:read", "linear:read", "notion:read"]);
  const [url, setUrl] = useState<string | null>(null);
  const action = useAction();
  const send = () =>
    email.trim() &&
    action.run(async () => {
      const result = await api.post("/invites", { email, role, grants: picked });
      setUrl(result.url);
    });

  if (url) {
    return (
      <Modal title="Invite link ready" onClose={onClose} footer={<Button variant="primary" onClick={onClose}>Done</Button>}>
        <p className="text-[13.5px]">Send this link to <strong className="font-semibold">{email}</strong>. Hyperbeam does not send email itself.</p>
        <CopyField value={url} label="Copy link" />
        <Notice tone="info">The link works once and expires in seven days. It will not be shown again.</Notice>
      </Modal>
    );
  }
  return (
    <Modal
      title="Invite someone"
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!email.trim()} loading={action.busy} onClick={() => void send()}>Create invite link</Button>
        </>
      }
    >
      <Field label="Email">
        {(id) => <Input id={id} type="email" autoFocus value={email} onChange={(e) => setEmail(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void send()} placeholder="teammate@company.com" />}
      </Field>
      <Field label="Role">
        {(id) => (
          <Select
            id={id}
            value={role}
            onValueChange={(v) => setRole(v as Role)}
            options={(["member", "admin"] as Role[]).map((r) => ({ value: r, label: <span className="capitalize">{r}</span>, description: ROLE_HELP[r] }))}
          />
        )}
      </Field>
      <div className="grid gap-2">
        <div>
          <div className="text-[13px] font-medium">Their agents may</div>
          <p className="text-[12.5px] text-muted">You can change these at any time.</p>
        </div>
        <div className="grid gap-2 sm:grid-cols-2">
          {grants.map((g) => (
            <CheckRow key={g.id} checked={picked.includes(g.id)} onCheckedChange={() => setPicked(toggleGrant(picked, g.id))} label={<span>{systemName(g.system)}: {g.label.toLowerCase()}</span>} />
          ))}
        </div>
      </div>
      <ErrorNote error={action.error} />
    </Modal>
  );
}

function ScopeDialog(props: { user: User; repos: Repo[]; onClose: () => void; onSave: (scope: string[] | null) => Promise<void> }) {
  const [all, setAll] = useState(props.user.repo_scope === null);
  const [picked, setPicked] = useState<string[]>(props.user.repo_scope ?? []);
  const [busy, setBusy] = useState(false);
  return (
    <Modal
      title={`Repositories for ${props.user.name}`}
      description="Their agents can only reach these, and only if the repository is registered."
      onClose={props.onClose}
      footer={
        <>
          <Button onClick={props.onClose}>Cancel</Button>
          <Button variant="primary" loading={busy} onClick={async () => { setBusy(true); await props.onSave(all ? null : picked); setBusy(false); }}>Save</Button>
        </>
      }
    >
      <div className="grid gap-2">
        {[true, false].map((value) => (
          <label key={String(value)} className={cn("flex cursor-pointer items-center gap-2.5 rounded-lg border px-3 py-2.5 text-[13.5px] transition-colors", all === value ? "border-primary/60 bg-accent/40" : "border-border hover:border-border-strong")}>
            <input type="radio" className="accent-[var(--primary)]" checked={all === value} onChange={() => setAll(value)} />
            {value ? "Every registered repository, including ones added later" : "Only the ones I pick"}
          </label>
        ))}
      </div>
      {!all && (
        <div className="grid gap-2 pl-1">
          {props.repos.length === 0 && <span className="text-[13px] text-muted">No repositories are registered yet.</span>}
          {props.repos.map((repo) => (
            <CheckRow
              key={repo.id}
              checked={picked.includes(repo.full_name)}
              onCheckedChange={() => setPicked(picked.includes(repo.full_name) ? picked.filter((r) => r !== repo.full_name) : [...picked, repo.full_name])}
              label={<span className="font-mono text-[12.5px]">{repo.full_name}</span>}
            />
          ))}
        </div>
      )}
    </Modal>
  );
}
