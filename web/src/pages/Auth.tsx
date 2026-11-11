import { type FormEvent, type ReactNode, useEffect, useState } from "react";
import { BrandMark, Wordmark } from "../components/ui/brand";
import { Button } from "../components/ui/button";
import { Field, Input } from "../components/ui/input";
import { Skeleton } from "../components/ui/surface";
import { api } from "../lib/api";
import { signedIn, useStore } from "../lib/store";
import { ErrorNote, navigate, useAction, useDocumentTitle, usePath } from "../lib/ui";

export function AuthScreen() {
  const meta = useStore((s) => s.meta)!;
  const path = usePath();
  const inviteToken = path.startsWith("/invite/") ? path.slice("/invite/".length) : null;
  return (
    <div className="relative grid min-h-full place-items-center overflow-hidden px-4 py-12">
      {/* A faint stain behind the card. */}
      <div aria-hidden className="pointer-events-none absolute left-1/2 top-[38%] size-[620px] -translate-x-1/2 -translate-y-1/2 rounded-full bg-primary/8 blur-[110px]" />
      <div className="relative grid w-full max-w-[400px] gap-6">
        <div className="flex items-center justify-center gap-2.5">
          <BrandMark size={32} />
          <Wordmark className="text-[24px]" />
        </div>
        {meta.needs_setup ? <Setup /> : inviteToken ? <AcceptInvite token={inviteToken} /> : <Login teamName={meta.team_name} />}
      </div>
    </div>
  );
}

function AuthCard(props: { title: string; description?: ReactNode; children: ReactNode; onSubmit?: (e: FormEvent) => void }) {
  useDocumentTitle(props.title);
  return (
    <form onSubmit={props.onSubmit} className="grid gap-4 rounded-xl border border-border bg-surface p-6 shadow-pop sm:p-7">
      <div className="grid gap-1">
        <h1 className="font-display text-[21px] font-semibold tracking-[-0.02em]">{props.title}</h1>
        {props.description && <p className="text-[13.5px] text-muted">{props.description}</p>}
      </div>
      {props.children}
    </form>
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
    <AuthCard title="Set up Noemune" description="You are the first person here, so you will own this installation. Invite the rest of your team once you are in." onSubmit={submit}>
      <Field label="Team name">{(id) => <Input id={id} required autoFocus placeholder="Acme" {...field("team_name")} />}</Field>
      <Field label="Your name">{(id) => <Input id={id} required autoComplete="name" {...field("name")} />}</Field>
      <Field label="Email">{(id) => <Input id={id} type="email" required autoComplete="email" {...field("email")} />}</Field>
      <Field label="Password" hint="At least 10 characters.">
        {(id) => <Input id={id} type="password" required minLength={10} autoComplete="new-password" {...field("password")} />}
      </Field>
      <ErrorNote error={action.error} />
      <Button type="submit" variant="primary" size="lg" loading={action.busy}>Create team</Button>
    </AuthCard>
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
    <>
      <AuthCard title={teamName ? `Sign in to ${teamName}` : "Sign in"} description="Where your team’s agents work in the open." onSubmit={submit}>
        <Field label="Email">{(id) => <Input id={id} type="email" required autoFocus autoComplete="email" value={email} onChange={(e) => setEmail(e.target.value)} />}</Field>
        <Field label="Password">{(id) => <Input id={id} type="password" required autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} />}</Field>
        <ErrorNote error={action.error} />
        <Button type="submit" variant="primary" size="lg" loading={action.busy}>Sign in</Button>
      </AuthCard>
      <p className="text-center text-[13px] text-muted">No account? Ask a team admin for an invite link.</p>
    </>
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
      <AuthCard title="This invite has expired" description="Invite links work once and last seven days. Ask a team admin to send you a new one.">
        <Button variant="secondary" size="lg" onClick={() => navigate("/")}>Go to sign in</Button>
      </AuthCard>
    );
  }
  if (!info) {
    return <div className="grid gap-4 rounded-xl border border-border bg-surface p-7"><Skeleton className="h-6 w-40" /><Skeleton className="h-9" /><Skeleton className="h-9" /><Skeleton className="h-10" /></div>;
  }

  const submit = (e: FormEvent) => {
    e.preventDefault();
    void action.run(async () => {
      const { user } = await api.post("/auth/accept-invite", { token, name, password });
      navigate("/");
      await signedIn(user);
    });
  };
  return (
    <AuthCard title={`Join ${info.team_name}`} description={<>You were invited as <strong className="font-semibold text-foreground">{info.email}</strong>.</>} onSubmit={submit}>
      <Field label="Your name">{(id) => <Input id={id} required autoFocus autoComplete="name" value={name} onChange={(e) => setName(e.target.value)} />}</Field>
      <Field label="Choose a password" hint="At least 10 characters.">
        {(id) => <Input id={id} type="password" required minLength={10} autoComplete="new-password" value={password} onChange={(e) => setPassword(e.target.value)} />}
      </Field>
      <ErrorNote error={action.error} />
      <Button type="submit" variant="primary" size="lg" loading={action.busy}>Join team</Button>
    </AuthCard>
  );
}
