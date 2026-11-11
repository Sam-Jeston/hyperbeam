import { useState } from "react";
import { api, type GrantInfo, type Repo, type Role, type User } from "../lib/api";
import { refreshMe, reloadUsers, useStore } from "../lib/store";
import { Avatar, ErrorNote, Modal, timeAgo, useAction, useLoad } from "../lib/ui";

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

export function TeamPage() {
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

  const update = (user: User, patch: Record<string, unknown>) =>
    action.run(async () => {
      await api.patch(`/users/${user.id}`, patch);
      await reloadUsers();
      if (user.id === me.id) await refreshMe();
    });

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>People &amp; permissions</h1>
          <p>
            An agent can do only what the person who started it is allowed to. Set that here, per person.
            {!isAdmin && " Only admins can change these."}
          </p>
        </div>
        {isAdmin && <button className="btn btn-primary" onClick={() => setInviting(true)}>Invite someone</button>}
      </div>
      <ErrorNote error={action.error} />

      <div className="card" style={{ overflowX: "auto" }}>
        <table className="matrix">
          <thead>
            <tr>
              <th rowSpan={2}>Person</th>
              <th rowSpan={2}>Role</th>
              {systems.map((system) => (
                <th key={system} className="system" colSpan={meta.grants.filter((g) => g.system === system).length}>{system}</th>
              ))}
              <th rowSpan={2}>Repositories</th>
              {isAdmin && <th rowSpan={2} />}
            </tr>
            <tr>
              {meta.grants.map((g, i) => (
                <th key={g.id} className={`grant ${i === 0 || meta.grants[i - 1].system !== g.system ? "first-of-system" : ""}`} title={g.label}>
                  {shortLabel(g)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {people.map((user) => {
              const locked = !isAdmin || (user.role === "owner" && me.role !== "owner");
              return (
                <tr key={user.id} style={user.disabled ? { opacity: 0.5 } : undefined}>
                  <td>
                    <div className="row">
                      <Avatar user={user} size={26} />
                      <div>
                        <div style={{ fontWeight: 500 }}>{user.name}{user.id === me.id && <span className="faint"> (you)</span>}</div>
                        <div className="small muted">{user.email}</div>
                      </div>
                    </div>
                  </td>
                  <td>
                    {locked || user.id === me.id || user.disabled ? (
                      <span style={{ textTransform: "capitalize" }}>{user.disabled ? "Removed" : user.role}</span>
                    ) : (
                      <select value={user.role} style={{ width: "auto" }} onChange={(e) => void update(user, { role: e.target.value })}>
                        <option value="member">Member</option>
                        <option value="admin">Admin</option>
                        {me.role === "owner" && <option value="owner">Owner</option>}
                      </select>
                    )}
                  </td>
                  {meta.grants.map((g, i) => (
                    <td key={g.id} className={`cell ${i === 0 || meta.grants[i - 1].system !== g.system ? "first-of-system" : ""}`}>
                      <input
                        type="checkbox"
                        aria-label={`${user.name}: ${g.label}`}
                        checked={user.grants.includes(g.id)}
                        disabled={locked || user.disabled || action.busy}
                        onChange={() => void update(user, { grants: toggleGrant(user.grants, g.id) })}
                      />
                    </td>
                  ))}
                  <td>
                    <button className="btn btn-sm btn-ghost" disabled={locked || user.disabled} onClick={() => setScoping(user)}>
                      {user.repo_scope === null ? "All" : `${user.repo_scope.length} selected`}
                    </button>
                  </td>
                  {isAdmin && (
                    <td style={{ textAlign: "right" }}>
                      {user.id !== me.id && !locked && (
                        <button className="btn btn-sm btn-ghost" onClick={() => void update(user, { disabled: !user.disabled })}>
                          {user.disabled ? "Restore" : "Remove"}
                        </button>
                      )}
                    </td>
                  )}
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <p className="small muted" style={{ marginTop: 10, maxWidth: "80ch" }}>
        Changes apply immediately, including to agents that are already running. Removing someone signs them out and stops their agents.
        Admins manage people, integrations and harnesses; an admin’s own agents are still limited to the permissions ticked here.
      </p>

      {isAdmin && invites.data && invites.data.invites.length > 0 && (
        <div className="section" style={{ marginTop: 26 }}>
          <h2 style={{ marginBottom: 10 }}>Pending invites</h2>
          <div className="card">
            <table>
              <tbody>
                {invites.data.invites.map((invite) => (
                  <tr key={invite.id}>
                    <td>{invite.email}</td>
                    <td className="muted" style={{ textTransform: "capitalize" }}>{invite.role}</td>
                    <td className="small muted">expires {timeAgo(invite.expires_at) === "just now" ? "soon" : new Date(invite.expires_at).toLocaleDateString()}</td>
                    <td style={{ textAlign: "right" }}>
                      <button className="btn btn-sm btn-ghost" onClick={() => void action.run(async () => (await api.del(`/invites/${invite.id}`), invites.reload()))}>Revoke</button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {inviting && <InviteDialog grants={meta.grants} onClose={() => (setInviting(false), invites.reload())} />}
      {scoping && (
        <ScopeDialog
          user={scoping}
          repos={repos.data?.repos ?? []}
          onClose={() => setScoping(null)}
          onSave={async (scope) => {
            await update(scoping, { repo_scope: scope });
            setScoping(null);
          }}
        />
      )}
    </div>
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
  const [copied, setCopied] = useState(false);
  const action = useAction();
  const send = () =>
    action.run(async () => {
      const result = await api.post("/invites", { email, role, grants: picked });
      setUrl(result.url);
    });

  if (url) {
    return (
      <Modal title="Invite link ready" onClose={onClose}>
        <p style={{ margin: 0 }}>Send this link to <strong>{email}</strong>. Noemune does not send email itself. The link works once and expires in seven days; it will not be shown again.</p>
        <input type="text" readOnly value={url} onFocus={(e) => e.target.select()} />
        <div className="modal-foot">
          <button className="btn" onClick={() => void navigator.clipboard.writeText(url).then(() => setCopied(true))}>{copied ? "Copied" : "Copy link"}</button>
          <button className="btn btn-primary" onClick={onClose}>Done</button>
        </div>
      </Modal>
    );
  }
  return (
    <Modal title="Invite someone" onClose={onClose}>
      <label className="field">Email<input type="email" autoFocus value={email} onChange={(e) => setEmail(e.target.value)} placeholder="teammate@company.com" /></label>
      <label className="field">
        Role
        <select value={role} onChange={(e) => setRole(e.target.value as Role)}>
          <option value="member">Member: starts agents and builds skills and workflows</option>
          <option value="admin">Admin: also manages people, integrations and harnesses</option>
        </select>
      </label>
      <div>
        <div className="field">Their agents may</div>
        <div className="stack" style={{ gap: 4, marginTop: 6 }}>
          {grants.map((g) => (
            <label key={g.id} className="check">
              <input type="checkbox" checked={picked.includes(g.id)} onChange={() => setPicked(toggleGrant(picked, g.id))} />
              <span><span style={{ textTransform: "capitalize" }}>{g.system}</span>: {g.label.toLowerCase()}</span>
            </label>
          ))}
        </div>
        <p className="hint" style={{ marginTop: 6 }}>You can change these at any time.</p>
      </div>
      <ErrorNote error={action.error} />
      <div className="modal-foot">
        <button className="btn" onClick={onClose}>Cancel</button>
        <button className="btn btn-primary" disabled={!email.trim() || action.busy} onClick={() => void send()}>Create invite link</button>
      </div>
    </Modal>
  );
}

function ScopeDialog(props: { user: User; repos: Repo[]; onClose: () => void; onSave: (scope: string[] | null) => Promise<void> }) {
  const [all, setAll] = useState(props.user.repo_scope === null);
  const [picked, setPicked] = useState<string[]>(props.user.repo_scope ?? []);
  return (
    <Modal title={`Repositories for ${props.user.name}`} onClose={props.onClose}>
      <label className="check"><input type="radio" checked={all} onChange={() => setAll(true)} /> Every registered repository, including ones added later</label>
      <label className="check"><input type="radio" checked={!all} onChange={() => setAll(false)} /> Only these</label>
      {!all && (
        <div className="stack" style={{ gap: 4, paddingLeft: 22 }}>
          {props.repos.length === 0 && <span className="small muted">No repositories are registered yet.</span>}
          {props.repos.map((repo) => (
            <label key={repo.id} className="check">
              <input
                type="checkbox"
                checked={picked.includes(repo.full_name)}
                onChange={() => setPicked(picked.includes(repo.full_name) ? picked.filter((r) => r !== repo.full_name) : [...picked, repo.full_name])}
              />
              {repo.full_name}
            </label>
          ))}
        </div>
      )}
      <div className="modal-foot">
        <button className="btn" onClick={props.onClose}>Cancel</button>
        <button className="btn btn-primary" onClick={() => void props.onSave(all ? null : picked)}>Save</button>
      </div>
    </Modal>
  );
}
