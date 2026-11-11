import { type FormEvent, useEffect, useState } from "react";
import { api } from "../lib/api";
import { signedIn, useStore } from "../lib/store";
import { ErrorNote, navigate, useAction, usePath } from "../lib/ui";

export function AuthScreen() {
  const meta = useStore((s) => s.meta)!;
  const path = usePath();
  const inviteToken = path.startsWith("/invite/") ? path.slice("/invite/".length) : null;
  return (
    <div className="auth">
      {meta.needs_setup ? <Setup /> : inviteToken ? <AcceptInvite token={inviteToken} /> : <Login teamName={meta.team_name} />}
    </div>
  );
}

function Setup() {
  const [form, setForm] = useState({ team_name: "", name: "", email: "", password: "" });
  const action = useAction();
  const field = (key: keyof typeof form) => ({
    value: form[key],
    onChange: (e: { target: { value: string } }) => setForm({ ...form, [key]: e.target.value }),
  });
  const submit = (e: FormEvent) => {
    e.preventDefault();
    void action.run(async () => {
      const { user } = await api.post("/setup", form);
      await signedIn(user);
      navigate("/settings/integrations");
    });
  };
  return (
    <form className="card auth-card" onSubmit={submit}>
      <div>
        <h1>Set up Noemune</h1>
        <p className="muted">You are the first person here, so you will own this installation. Invite the rest of your team once you are in.</p>
      </div>
      <label className="field">Team name<input type="text" required autoFocus placeholder="Acme" {...field("team_name")} /></label>
      <label className="field">Your name<input type="text" required autoComplete="name" {...field("name")} /></label>
      <label className="field">Email<input type="email" required autoComplete="email" {...field("email")} /></label>
      <label className="field">
        Password
        <input type="password" required minLength={10} autoComplete="new-password" {...field("password")} />
        <span className="hint">At least 10 characters.</span>
      </label>
      <ErrorNote error={action.error} />
      <button className="btn btn-primary" disabled={action.busy}>{action.busy ? "Creating…" : "Create team"}</button>
    </form>
  );
}

function Login({ teamName }: { teamName: string | null }) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const action = useAction();
  const submit = (e: FormEvent) => {
    e.preventDefault();
    void action.run(async () => {
      const { user } = await api.post("/auth/login", { email, password });
      await signedIn(user);
    });
  };
  return (
    <form className="card auth-card" onSubmit={submit}>
      <div>
        <h1>Sign in{teamName ? ` to ${teamName}` : ""}</h1>
        <p className="muted">Noemune is where your team’s agents work in the open.</p>
      </div>
      <label className="field">Email<input type="email" required autoFocus autoComplete="email" value={email} onChange={(e) => setEmail(e.target.value)} /></label>
      <label className="field">Password<input type="password" required autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} /></label>
      <ErrorNote error={action.error} />
      <button className="btn btn-primary" disabled={action.busy}>{action.busy ? "Signing in…" : "Sign in"}</button>
      <p className="small muted">No account? Ask a team admin for an invite link.</p>
    </form>
  );
}

function AcceptInvite({ token }: { token: string }) {
  const [info, setInfo] = useState<{ email: string; team_name: string } | null>(null);
  const [invalid, setInvalid] = useState(false);
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const action = useAction();

  useEffect(() => {
    api.get(`/invite-info/${token}`).then(setInfo, () => setInvalid(true));
  }, [token]);

  if (invalid) {
    return (
      <div className="card auth-card">
        <h1>This invite has expired</h1>
        <p className="muted">Invite links work once and last seven days. Ask a team admin to send you a new one.</p>
        <button className="btn" onClick={() => navigate("/")}>Go to sign in</button>
      </div>
    );
  }
  if (!info) return null;

  const submit = (e: FormEvent) => {
    e.preventDefault();
    void action.run(async () => {
      const { user } = await api.post("/auth/accept-invite", { token, name, password });
      navigate("/");
      await signedIn(user);
    });
  };
  return (
    <form className="card auth-card" onSubmit={submit}>
      <div>
        <h1>Join {info.team_name}</h1>
        <p className="muted">You were invited as <strong>{info.email}</strong>.</p>
      </div>
      <label className="field">Your name<input type="text" required autoFocus autoComplete="name" value={name} onChange={(e) => setName(e.target.value)} /></label>
      <label className="field">
        Choose a password
        <input type="password" required minLength={10} autoComplete="new-password" value={password} onChange={(e) => setPassword(e.target.value)} />
        <span className="hint">At least 10 characters.</span>
      </label>
      <ErrorNote error={action.error} />
      <button className="btn btn-primary" disabled={action.busy}>{action.busy ? "Joining…" : "Join team"}</button>
    </form>
  );
}
