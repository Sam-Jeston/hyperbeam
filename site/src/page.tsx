import {
  ArrowRight, BookOpen, Bot, Check, CircleCheck, Columns3, Copy, FilePen, FileText, GitPullRequest, Globe, KeyRound, Loader2, Menu, Moon,
  Network, NotebookPen, NotebookText, Package, ScrollText, Server, ShieldCheck, ShieldX, SquareKanban, Sun, Terminal, Users, Workflow,
} from "lucide-react";
import type { ReactNode } from "react";
import { MARK_FILLS, MARK_LAYERS } from "../../brand/mark";
import { docs, REPO_URL } from "./config";
import { AGENTS, BEATS, clock, DESTS, LANES, logRow, PEOPLE, type DestId } from "./egress-data";
import { highlight } from "./highlight";

const cx = (...c: (string | false | null | undefined)[]) => c.filter(Boolean).join(" ");

// ------------------------------------------------------------------- brand

export function BrandMark({ size = 24, className }: { size?: number; className?: string }) {
  return (
    <svg viewBox="0 0 16 16" width={size} height={size} shapeRendering="crispEdges" className={cx("shrink-0", className)} aria-hidden="true">
      {MARK_FILLS.map(([layer, fill]) => <path key={layer} d={MARK_LAYERS[layer]} fill={fill} />)}
    </svg>
  );
}

function Wordmark({ className }: { className?: string }) {
  return <span className={cx("font-display font-bold tracking-[-0.035em]", className)}>hyperbeam</span>;
}

function GitHubIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 16 16" width="16" height="16" className={className} aria-hidden="true" fill="currentColor">
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z" />
    </svg>
  );
}

// --------------------------------------------------------------- primitives

function ButtonLink(props: { href: string; children: ReactNode; variant?: "primary" | "secondary" | "ghost"; className?: string }) {
  return (
    <a
      href={props.href}
      className={cx(
        "inline-flex h-10 items-center justify-center gap-2 whitespace-nowrap rounded-md px-4 text-[14px] font-medium transition-colors [&_svg]:size-4",
        "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring",
        props.variant === "secondary" ? "border border-border-strong bg-surface text-foreground hover:bg-surface-2" :
        props.variant === "ghost" ? "text-muted hover:bg-surface-2 hover:text-foreground" :
        "bg-primary text-primary-foreground shadow-soft hover:bg-primary-hover",
        props.className,
      )}
    >
      {props.children}
    </a>
  );
}

function Section(props: { id?: string; children: ReactNode; className?: string }) {
  return (
    <section id={props.id} className={cx("px-4 py-20 sm:px-8 sm:py-28", props.className)}>
      <div className="mx-auto max-w-[1160px]">{props.children}</div>
    </section>
  );
}

function SectionHead(props: { eyebrow: string; title: ReactNode; children?: ReactNode; center?: boolean }) {
  return (
    <div className={cx("mb-12 grid max-w-[680px] gap-3", props.center && "mx-auto text-center")}>
      <div className="eyebrow">{props.eyebrow}</div>
      <h2 className="font-display text-[30px] font-semibold leading-[1.12] tracking-[-0.025em] sm:text-[38px]">{props.title}</h2>
      {props.children && <p className="text-[16px] leading-relaxed text-muted">{props.children}</p>}
    </div>
  );
}

function CodeBlock({ code, lang, title }: { code: string; lang: string; title?: string }) {
  return (
    <div className="code overflow-hidden rounded-lg border border-border bg-surface">
      {title && (
        <div className="flex items-center justify-between border-b border-border px-4 py-2 font-mono text-[12px] text-muted">
          {title}
          <button type="button" data-copy={code} className="inline-flex items-center gap-1.5 rounded-md px-2 py-1 font-sans text-[12px] text-muted hover:bg-surface-2 hover:text-foreground">
            <Copy className="size-3.5" /> <span>Copy</span>
          </button>
        </div>
      )}
      <div dangerouslySetInnerHTML={{ __html: highlight(code, lang) }} />
    </div>
  );
}

// ---------------------------------------------------------------- sections

function Nav() {
  const links = [
    { href: "#how", label: "How it works" },
    { href: "#security", label: "Security" },
    { href: "#self-host", label: "Self-host" },
    { href: docs(), label: "Docs" },
  ];
  return (
    <header className="sticky top-0 z-40 border-b border-border/70 bg-background/80 backdrop-blur-md">
      <div className="mx-auto flex h-16 max-w-[1160px] items-center gap-6 px-4 sm:px-8">
        <a href="/" className="flex items-center gap-2.5" aria-label="Hyperbeam home">
          <BrandMark size={26} />
          <Wordmark className="text-[19px]" />
        </a>
        <nav className="hidden items-center gap-1 md:flex" aria-label="Main">
          {links.map((l) => (
            <a key={l.href} href={l.href} className="rounded-md px-3 py-1.5 text-[14px] text-muted transition-colors hover:text-foreground">{l.label}</a>
          ))}
        </nav>
        <div className="ml-auto flex items-center gap-2">
          {REPO_URL && (
            <a href={REPO_URL} className="hidden size-9 place-items-center rounded-md text-muted hover:bg-surface-2 hover:text-foreground sm:grid" aria-label="Source on GitHub"><GitHubIcon /></a>
          )}
          <button type="button" data-theme-toggle className="grid size-9 place-items-center rounded-md text-muted hover:bg-surface-2 hover:text-foreground" aria-label="Switch between light and dark">
            <Sun className="hidden size-4 [.dark_&]:block" />
            <Moon className="hidden size-4 [.light_&]:block" />
          </button>
          <ButtonLink href={docs("quickstart")} className="hidden h-9 sm:inline-flex">Get started</ButtonLink>
          <button type="button" data-menu-toggle aria-expanded="false" aria-controls="mobile-nav" className="grid size-9 place-items-center rounded-md text-muted hover:bg-surface-2 md:hidden" aria-label="Menu">
            <Menu className="size-5" />
          </button>
        </div>
      </div>
      <nav id="mobile-nav" hidden className="border-t border-border px-4 py-3 md:hidden" aria-label="Main">
        {links.map((l) => <a key={l.href} href={l.href} className="block rounded-md px-3 py-2 text-[15px] text-muted hover:bg-surface-2 hover:text-foreground">{l.label}</a>)}
        <ButtonLink href={docs("quickstart")} className="mt-2 w-full">Get started</ButtonLink>
      </nav>
    </header>
  );
}

function Hero() {
  return (
    <section className="relative overflow-hidden px-4 pt-16 pb-20 sm:px-8 sm:pt-24 sm:pb-28">
      <div aria-hidden="true" className="dot-grid pointer-events-none absolute inset-0" />
      <div aria-hidden="true" className="pointer-events-none absolute left-1/2 top-[28%] size-[640px] -translate-x-1/2 rounded-full bg-primary/10 blur-[120px]" />
      <HeroBeam />
      <div className="relative mx-auto max-w-[1160px]">
        <div className="mx-auto grid max-w-[820px] justify-items-center gap-6 text-center">
          <a href={docs("concepts")} className="inline-flex items-center gap-2 rounded-full border border-border-strong bg-surface/70 py-1 pr-3 pl-1.5 text-[13px] text-muted backdrop-blur hover:text-foreground">
            <span className="rounded-full bg-accent px-2 py-0.5 text-[12px] font-medium text-accent-foreground">Self-hosted</span>
            Open source · Any agent that speaks ACP
            <ArrowRight className="size-3.5" />
          </a>
          <h1 className="font-display text-[42px] font-semibold leading-[1.04] tracking-[-0.035em] text-balance sm:text-[64px]">
            Your team’s coding agents, <span className="text-primary">working in the open.</span>
          </h1>
          <p className="max-w-[620px] text-[17px] leading-relaxed text-muted sm:text-[19px]">
            Describe an outcome. An agent picks it up in its own sandbox, and everyone on the team can watch the scoping and the building as it happens.
          </p>
          <div className="flex flex-wrap justify-center gap-3">
            <ButtonLink href={docs("quickstart")}>Run it on your laptop <ArrowRight /></ButtonLink>
            <ButtonLink href={docs()} variant="secondary"><BookOpen /> Read the docs</ButtonLink>
          </div>
        </div>
        <div className="mt-16 sm:mt-20"><ProductDemo /></div>
      </div>
    </section>
  );
}

/** The mark scaled up so each pixel is a block, fired into the hero from its top right corner. Wide screens only, where it clears the headline. */
function HeroBeam() {
  return (
    <svg viewBox="0 0 16 16" shapeRendering="crispEdges" aria-hidden="true" className="hero-beam pointer-events-none absolute top-6 right-[max(-24px,calc(50%-720px))] hidden size-[320px] lg:block xl:size-[384px]">
      {MARK_FILLS.map(([layer, fill]) => <path key={layer} d={MARK_LAYERS[layer]} fill={fill} />)}
    </svg>
  );
}

/** A session as it appears in Hyperbeam. The script plays it step by step; without it, it reads as a finished session. */
function ProductDemo() {
  const step = (n: number) => ({ "data-step": n });
  return (
    <div data-demo className="relative mx-auto max-w-[1040px] overflow-hidden rounded-xl border border-border-strong bg-surface shadow-pop">
      <div className="flex items-center gap-3 border-b border-border bg-surface-2/50 px-4 py-2.5">
        <div className="flex gap-1.5" aria-hidden="true">{[0, 1, 2].map((i) => <span key={i} className="size-2.5 rounded-full bg-border-strong" />)}</div>
        <div className="truncate text-[12.5px] text-faint">Board › Export the audit log as CSV</div>
      </div>
      <div className="grid md:grid-cols-[minmax(0,1fr)_260px]">
        <div className="grid min-w-0 content-start gap-3 p-5 text-left sm:p-6">
          <div className="flex flex-wrap items-center gap-2.5">
            <h3 className="font-display text-[19px] font-semibold tracking-[-0.015em]">Add CSV export to audit</h3>
            <span className="inline-flex items-center gap-1.5 rounded-full bg-run/14 px-2 py-0.5 text-[12px] font-medium text-run">
              <span className="size-1.5 rounded-full bg-run animate-pulse-ring" /> Working
            </span>
          </div>
          <div {...step(1)} className="ml-auto max-w-[85%] rounded-2xl rounded-tr-md bg-accent px-3.5 py-2 text-[14px] leading-relaxed">
            Add a CSV export to the audit log. Keep the JSON endpoint as it is.
          </div>
          <Tool {...step(2)} icon={<FileText />} title="Read crates/server/src/api/audit.rs" />
          <Tool {...step(3)} icon={<FilePen />} title="Edit crates/server/src/api/audit.rs" diff="+64 −3" />
          <Tool {...step(4)} icon={<Terminal />} title="cargo test -p hyperbeam-server" />
          <Gate {...step(5)} allowed text="github: push to nm/audit-csv" />
          <Gate {...step(6)} allowed={false} text="egress to pastebin.com" />
          <div {...step(7)} className="text-[14px] leading-relaxed">
            The export is at <code className="rounded border border-border bg-surface-2 px-1 font-mono text-[12.5px]">/api/v1/audit.csv</code> and streams rows, so large logs don’t load into memory. Tests pass.
            <a href="#how" className="mt-2 flex w-fit items-center gap-1.5 rounded-md border border-border bg-surface-2/60 px-2.5 py-1 text-[13px] text-primary">
              <GitPullRequest className="size-3.5" /> acme/api#418 · Add CSV export to the audit log
            </a>
          </div>
        </div>
        <aside className="grid content-start gap-5 border-t border-border bg-surface-2/25 p-5 text-left text-[13px] md:border-t-0 md:border-l">
          <div>
            <div className="mb-2.5 text-[11px] font-semibold uppercase tracking-[0.07em] text-faint">Plan</div>
            <ul className="grid gap-2">
              {["Read the audit endpoint", "Add a streaming CSV writer", "Run the server tests", "Open a pull request"].map((p, i) => (
                <li key={p} {...step(i * 2 + 2)} className="flex items-start gap-2">
                  {i < 3 ? <CircleCheck className="mt-px size-4 shrink-0 text-run" /> : <Loader2 className="mt-px size-4 shrink-0 animate-spin text-primary" />}
                  <span className={i < 3 ? "text-muted line-through decoration-faint" : "font-medium"}>{p}</span>
                </li>
              ))}
            </ul>
          </div>
          <div>
            <div className="mb-2.5 text-[11px] font-semibold uppercase tracking-[0.07em] text-faint">May do</div>
            <div className="flex flex-wrap gap-1.5">
              {["github:read", "github:write", "github:open_pr"].map((g) => <span key={g} className="rounded-full border border-border-strong px-2 py-0.5 font-mono text-[11px] text-muted">{g}</span>)}
            </div>
          </div>
          <div>
            <div className="mb-2.5 text-[11px] font-semibold uppercase tracking-[0.07em] text-faint">Watching</div>
            <div className="flex -space-x-1.5">
              {[["PK", 282], ["JM", 160], ["AL", 30]].map(([n, h]) => (
                <span key={n} className="grid size-6 place-items-center rounded-full text-[10px] font-semibold text-white ring-2 ring-surface" style={{ background: `oklch(0.58 0.12 ${h})` }}>{n}</span>
              ))}
            </div>
          </div>
        </aside>
      </div>
    </div>
  );
}

function Tool(props: { icon: ReactNode; title: string; diff?: string; "data-step"?: number }) {
  return (
    <div data-step={props["data-step"]} className="flex min-w-0 items-center gap-2.5 rounded-lg border border-border bg-background/40 px-3 py-2 text-[13px] [&>svg:first-child]:size-4 [&>svg:first-child]:shrink-0 [&>svg:first-child]:text-faint">
      {props.icon}
      <span className="min-w-0 flex-1 truncate font-mono text-[12.5px]">{props.title}</span>
      {props.diff && <span className="font-mono text-[11.5px]"><span className="text-run">{props.diff.split(" ")[0]}</span> <span className="text-fail">{props.diff.split(" ")[1]}</span></span>}
      <CircleCheck className="size-4 shrink-0 text-run" />
    </div>
  );
}

function Gate(props: { allowed: boolean; text: string; "data-step"?: number }) {
  return (
    <div data-step={props["data-step"]} className={cx("flex w-fit max-w-full items-center gap-1.5 rounded-md border px-2.5 py-1 text-[12.5px]", props.allowed ? "border-border bg-surface-2/60 text-muted" : "border-fail/30 bg-fail/8 text-fail")}>
      {props.allowed ? <ShieldCheck className="size-3.5 shrink-0 text-run" /> : <ShieldX className="size-3.5 shrink-0" />}
      <span className="font-medium">{props.allowed ? "Allowed" : "Blocked"}</span>
      <span className="truncate">{props.text}</span>
    </div>
  );
}

function Harnesses() {
  const names = ["Claude Code", "Codex", "Gemini CLI", "OpenCode"];
  return (
    <div className="border-y border-border bg-surface/40 px-4 py-8 sm:px-8">
      <div className="mx-auto flex max-w-[1160px] flex-wrap items-center justify-center gap-x-10 gap-y-4">
        <span className="text-[13px] text-faint">Runs the agents your team already uses</span>
        {names.map((n) => <span key={n} className="font-display text-[17px] font-semibold tracking-[-0.01em] text-muted">{n}</span>)}
        <a href={docs("harnesses")} className="text-[13.5px] text-primary hover:underline">or anything that speaks ACP →</a>
      </div>
    </div>
  );
}

function HowItWorks() {
  const steps = [
    {
      title: "Describe the outcome",
      body: "Anyone on the team writes a brief from the board, the way they would brief a colleague. Pick the repositories, the agent, the model, and what it may do.",
      visual: (
        <div className="grid gap-2 text-[13px]">
          <div className="rounded-md border border-border-strong bg-background/50 px-3 py-2 leading-relaxed text-muted">Fix the flaky checkout test. Don’t change the payment mocks. Done when CI passes ten runs in a row.</div>
          <div className="flex flex-wrap gap-1.5">
            {["acme/web", "Claude Code", "github:open_pr"].map((x) => <span key={x} className="rounded-full border border-primary/50 bg-accent px-2 py-0.5 font-mono text-[11px] text-accent-foreground">{x}</span>)}
          </div>
        </div>
      ),
    },
    {
      title: "An agent works in its own sandbox",
      body: "Each session gets a fresh, locked-down sandbox with only what it needs. Every tool call, file edit and blocked request is streamed to the page as it happens.",
      visual: (
        <div className="grid gap-1.5">
          <Tool icon={<Terminal />} title="npm test -- checkout" />
          <Gate allowed text="github: push to nm/flaky-checkout" />
        </div>
      ),
    },
    {
      title: "The team reviews together",
      body: "Workstreams move from scoping to building to review. Reply to an agent, add another one, discuss it with the team, and merge the pull request when it is right.",
      visual: (
        <div className="grid grid-cols-3 gap-1.5 text-[11.5px]">
          {[["Scoping", "bg-idle"], ["Building", "bg-run"], ["In review", "bg-wait"]].map(([s, dot], i) => (
            <div key={s} className="rounded-md bg-surface-2/60 p-1.5">
              <div className="mb-1.5 flex items-center gap-1 font-semibold"><span className={cx("size-1.5 rounded-full", dot)} />{s}</div>
              <div className={cx("h-7 rounded border border-border bg-surface", i === 2 && "border-primary/50")} />
            </div>
          ))}
        </div>
      ),
    },
  ];
  return (
    <Section id="how">
      <SectionHead eyebrow="How it works" title="Agent work, moved out of one person’s terminal">
        Most agent work happens on someone’s laptop and stays invisible until a pull request appears. Hyperbeam moves it one level up, where the whole team can see it.
      </SectionHead>
      <ol className="grid gap-4 md:grid-cols-3">
        {steps.map((s, i) => (
          <li key={s.title} className="grid content-start gap-5 rounded-xl border border-border bg-surface p-6">
            <div className="flex items-center gap-3">
              <span className="grid size-7 place-items-center rounded-full bg-accent font-display text-[13px] font-bold text-accent-foreground">{i + 1}</span>
              <h3 className="font-display text-[17px] font-semibold tracking-[-0.01em]">{s.title}</h3>
            </div>
            <p className="text-[14.5px] leading-relaxed text-muted">{s.body}</p>
            <div className="mt-auto">{s.visual}</div>
          </li>
        ))}
      </ol>
    </Section>
  );
}

function Features() {
  const items = [
    { icon: <Columns3 />, title: "A shared board", body: "Every workstream, the agents on it, what each is doing right now, and the pull requests they produced. Live, for everyone." },
    { icon: <Network />, title: "A conductor", body: "One agent scopes a request, starts worker agents across repositories, and is told as each reports back." },
    { icon: <Bot />, title: "Any harness, any model", body: "Claude Code, Codex, Gemini CLI, OpenCode or your own ACP agent. Pick the model per session, or set a team default." },
    { icon: <BookOpen />, title: "Skills", body: "How your team writes migrations, runs tests and reviews code, in the open SKILL.md format. Installed into every sandbox that needs them." },
    { icon: <NotebookPen />, title: "Team notes", body: "Durable knowledge agents read and add to as they learn, so the next session starts out knowing more." },
    { icon: <Terminal />, title: "A command line", body: "Start, follow, message and stop agents from your terminal or CI, with the same permissions as the UI." },
  ];
  return (
    <Section className="bg-surface/30">
      <SectionHead eyebrow="For the whole team" title="Everything around the agent, in one place">
        Hyperbeam doesn’t replace your coding agent. It gives it a place to work that your team can see, steer and trust.
      </SectionHead>
      <div className="grid gap-px overflow-hidden rounded-xl border border-border bg-border sm:grid-cols-2 lg:grid-cols-3">
        {items.map((f) => (
          <div key={f.title} className="grid content-start gap-2.5 bg-background p-6">
            <div className="grid size-9 place-items-center rounded-lg bg-accent text-accent-foreground [&_svg]:size-[18px]">{f.icon}</div>
            <h3 className="mt-1 font-display text-[16px] font-semibold tracking-[-0.01em]">{f.title}</h3>
            <p className="text-[14px] leading-relaxed text-muted">{f.body}</p>
          </div>
        ))}
      </div>
    </Section>
  );
}

function Security() {
  const grants = [
    ["github:read", "clone registered repositories; read issues and pull requests"],
    ["github:write", "push branches (not the default branch); comment; create issues"],
    ["github:open_pr", "open pull requests"],
    ["github:merge_pr", "merge pull requests; push to the default branch"],
    ["linear:read · write", "read, or create, update and comment on issues"],
    ["notion:read · write", "read, or create and append to pages"],
  ];
  return (
    <Section id="security">
      <SectionHead eyebrow="Security" title="Agents never hold your credentials">
        A sandbox holds exactly one secret: a run token that identifies its session, revoked the moment the session ends. Everything it does outside the sandbox goes through Hyperbeam, which checks it and records it.
      </SectionHead>

      <EgressScene />

      <div className="mt-12 grid gap-10 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.15fr)]">
        <div className="grid content-start gap-4">
          <h3 className="font-display text-[22px] font-semibold tracking-[-0.02em]">Permissions belong to people</h3>
          <p className="text-[15px] leading-relaxed text-muted">
            An agent can do only what the person who started it may, per repository. Grants are checked against what that person holds now, so removing a permission takes effect on their running agents immediately.
          </p>
          <ul className="grid gap-2.5 text-[14.5px]">
            {[
              "A worker started by a conductor holds at most what the conductor holds.",
              "Anyone can watch a session. Only people who hold its grants can direct it.",
              "Credentials are encrypted at rest and never enter a sandbox.",
            ].map((t) => <li key={t} className="flex gap-2.5"><Check className="mt-0.5 size-4 shrink-0 text-run" />{t}</li>)}
          </ul>
          <a href={docs("security")} className="mt-2 inline-flex items-center gap-1.5 text-[14px] text-primary hover:underline">How the security model works <ArrowRight className="size-3.5" /></a>
        </div>
        <div className="overflow-hidden rounded-xl border border-border">
          <table className="w-full border-collapse text-left text-[13.5px]">
            <thead className="bg-surface-2/50 text-[12px] text-muted">
              <tr><th className="px-4 py-2.5 font-medium">Grant</th><th className="px-4 py-2.5 font-medium">Lets an agent</th></tr>
            </thead>
            <tbody className="divide-y divide-border">
              {grants.map(([g, d]) => (
                <tr key={g}><td className="whitespace-nowrap px-4 py-2.5 font-mono text-[12.5px] text-accent-foreground">{g}</td><td className="px-4 py-2.5 text-muted">{d}</td></tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </Section>
  );
}

/**
 * Requests leaving agent sandboxes and meeting Hyperbeam: let through to the system they asked for, or refused at
 * the gate. Each agent carries the grants of the person who started it. The script animates it (egress.ts);
 * without it, it reads as a diagram with a few lines of audit log.
 */
/** How a card in the egress scene lights up when the script adds a state class to it. */
const LIGHTS = cx(
  "transition-[border-color,box-shadow,color] duration-200",
  "[&.is-sending]:border-primary/70 [&.is-sending]:ring-3 [&.is-sending]:ring-primary/15",
  "[&.is-hit]:border-run/70 [&.is-hit]:ring-3 [&.is-hit]:ring-run/15 [&.is-granted]:border-run/60",
  "[&.is-blocked]:border-fail/70 [&.is-blocked]:ring-3 [&.is-blocked]:ring-fail/15",
  "[&.is-refused]:border-fail/70 [&.is-refused]:ring-3 [&.is-refused]:ring-fail/15 [&.is-refused]:text-fail",
  "[&.is-revoking]:border-wait/70 [&.is-revoking]:ring-3 [&.is-revoking]:ring-wait/20",
);

function EgressScene() {
  const destIcon: Record<DestId, ReactNode> = {
    github: <GitHubIcon />, linear: <SquareKanban />, notion: <NotebookText />, npm: <Package />, pastebin: <Globe />, metadata: <Globe />,
  };
  // Five rows, as many as the live log keeps, so the figure doesn't change height when the script takes over.
  const staticLog = BEATS.slice(0, 5).map((b, i) => logRow(b, clock(i * 3))).reverse().join("");
  const colLabel = "mb-3 text-[11px] font-semibold uppercase tracking-[0.07em] text-faint";
  return (
    <figure data-egress className="relative overflow-hidden rounded-xl border border-border bg-surface/40">
      <div aria-hidden="true" className="dot-grid pointer-events-none absolute inset-0 opacity-70" />
      <canvas data-egress-back aria-hidden="true" className="pointer-events-none absolute inset-0 size-full" />
      <div className="relative z-10 grid gap-y-16 p-5 sm:p-8 lg:grid-cols-[minmax(0,1fr)_216px_minmax(0,0.78fr)] lg:gap-x-[clamp(72px,9vw,128px)]">
        <div data-agents>
          <div className={colLabel}>Agents, in their sandboxes</div>
          <div className="grid gap-2.5 sm:grid-cols-2 lg:grid-cols-1">
            {AGENTS.map((a) => (
              <div key={a.id} data-agent={a.id} data-owner={a.owner} className={cx(LIGHTS, "grid gap-2 rounded-lg border border-border bg-surface p-3")}>
                <div className="flex items-center gap-2">
                  <Bot className="size-4 shrink-0 text-faint" />
                  <span className="text-[13.5px] font-semibold">{a.harness}</span>
                  <span className="ml-auto truncate font-mono text-[11px] text-faint">{a.repo}</span>
                </div>
                <div className="flex items-center gap-1.5 text-[12px] text-muted">
                  <span className="grid size-4.5 place-items-center rounded-full text-[8.5px] font-semibold text-white" style={{ background: `oklch(0.58 0.12 ${PEOPLE[a.owner].hue})` }}>{a.owner}</span>
                  <span>for {PEOPLE[a.owner].name}</span>
                  {a.via && <span className="truncate text-faint">· {a.via}</span>}
                </div>
                <div className="flex flex-wrap gap-1">
                  {a.grants.map((g) => <span key={g} data-grant={g} className="rounded-full border border-border-strong px-1.5 py-px font-mono text-[10.5px] text-muted transition-colors [&.revoked]:border-fail/45 [&.revoked]:bg-fail/10 [&.revoked]:text-fail [&.revoked]:line-through">{g}</span>)}
                </div>
              </div>
            ))}
          </div>
        </div>

        <div data-gate className="relative grid content-center gap-3 self-center rounded-xl border border-primary/50 bg-surface p-4 shadow-pop">
          <div className="flex items-center gap-2">
            <BrandMark size={20} />
            <Wordmark className="text-[16px]" />
          </div>
          <div className="grid grid-cols-3 gap-1.5 lg:grid-cols-1 lg:gap-2">
            {LANES.map((l) => (
              <div key={l.id} data-lane={l.id} className={cx(LIGHTS, "rounded-md border border-border bg-background/60 px-2.5 py-2")}>
                <div className="text-[12.5px] font-medium leading-tight">{l.label}</div>
                <div className="mt-0.5 hidden font-mono text-[10.5px] text-faint sm:block">{l.hint}</div>
              </div>
            ))}
          </div>
          <div className="flex items-center gap-1.5 text-[11.5px] text-muted">
            <KeyRound className="size-3.5 shrink-0 text-primary" /> Credentials are added here, never inside the sandbox
          </div>
        </div>

        <div data-dests>
          <div className={colLabel}>Your systems and the internet</div>
          <div className="grid grid-cols-2 gap-2 lg:grid-cols-1">
            {DESTS.map((d) => (
              <div
                key={d.id}
                data-dest={d.id}
                className={cx(
                  LIGHTS,
                  "flex min-w-0 items-center gap-2 rounded-lg border px-3 py-2 text-[13px] [&_svg]:size-4 [&_svg]:shrink-0",
                  d.allowed ? "border-border bg-surface [&_svg]:text-muted" : "border-dashed border-border-strong bg-background/70 text-faint",
                )}
              >
                {destIcon[d.id]}
                <span className={cx("truncate", (d.id === "npm" || !d.allowed) && "font-mono text-[11.5px]")}>{d.label}</span>
              </div>
            ))}
          </div>
        </div>
      </div>
      <canvas data-egress-front aria-hidden="true" className="pointer-events-none absolute inset-0 z-20 size-full" />
      <figcaption className="relative z-10 border-t border-border bg-background/60 px-5 py-3 sm:px-8">
        <div className="mb-1.5 flex items-center gap-2 text-[11px] font-semibold uppercase tracking-[0.07em] text-faint">
          <ScrollText className="size-3.5" /> Audit log
        </div>
        <ol data-egress-log aria-label="Audit log, illustrative" className="grid" dangerouslySetInnerHTML={{ __html: staticLog }} />
      </figcaption>
    </figure>
  );
}

const WORKFLOW = `name: ship-feature
description: Build a feature across the API and the web app
role: conductor
repos: [acme/api, acme/web]
skills: [house-style]
grants: [github:write, github:open_pr, linear:read]
inputs:
  - name: feature
    description: What to build, and how you will know it works
prompt: |
  Build this feature: {{feature}}
  Scope it first, then delegate each repository's changes to a worker.
  Open a pull request per repository and report the links.
`;

const CLI = `hyperbeam run "Fix the flaky checkout test" --repo acme/web --model sonnet --follow
hyperbeam workflow ship-feature -i feature="CSV export" --follow
hyperbeam ls
hyperbeam send 3ff6c49b "Also update the changelog" --follow
hyperbeam stop 3ff6c49b
`;

function Repeatable() {
  return (
    <Section className="bg-surface/30">
      <div className="grid items-center gap-12 lg:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]">
        <div className="min-w-0">
          <SectionHead eyebrow="Workflows and CLI" title="Turn a good request into a button">
            A workflow is a saved, parameterised launch: which repositories, skills and permissions, and what to ask for. Anyone can run it from the board by filling in the blanks, or from CI with the command line.
          </SectionHead>
          <div className="-mt-4 flex flex-wrap gap-3">
            <ButtonLink href={docs("workflows")} variant="secondary"><Workflow /> Workflow format</ButtonLink>
            <ButtonLink href={docs("cli")} variant="ghost"><Terminal /> CLI reference</ButtonLink>
          </div>
        </div>
        <div data-tabs className="min-w-0">
          <div role="tablist" aria-label="Examples" className="mb-3 flex gap-1">
            {[["workflow", "ship-feature.yaml"], ["cli", "Terminal"]].map(([id, label], i) => (
              <button
                key={id}
                type="button"
                role="tab"
                id={`tab-${id}`}
                aria-controls={`panel-${id}`}
                aria-selected={i === 0}
                className="rounded-md px-3 py-1.5 font-mono text-[12.5px] text-muted aria-selected:bg-surface-2 aria-selected:text-foreground hover:text-foreground"
              >
                {label}
              </button>
            ))}
          </div>
          <div role="tabpanel" id="panel-workflow" aria-labelledby="tab-workflow"><CodeBlock code={WORKFLOW} lang="yaml" title="ship-feature.yaml" /></div>
          <div role="tabpanel" id="panel-cli" aria-labelledby="tab-cli" hidden><CodeBlock code={CLI} lang="bash" title="Terminal" /></div>
        </div>
      </div>
    </Section>
  );
}

const HELM = `helm install hyperbeam deploy/helm/hyperbeam \\
  --namespace hyperbeam --create-namespace \\
  --set masterKey="$(openssl rand -base64 32)" \\
  --set publicUrl=https://hyperbeam.example.com`;

function SelfHost() {
  const points: [ReactNode, string, string][] = [
    [<Server />, "Your cluster, your data", "The control plane and Postgres run in your Kubernetes cluster. Nothing leaves it except what your agents are allowed to reach."],
    [<ShieldCheck />, "Locked-down sandboxes", "One pod per live session, non-root, no service account token, no added capabilities, under the restricted Pod Security Standard."],
    [<Loader2 />, "Idle sandboxes go away", "Sandboxes are torn down after 15 idle minutes. A message wakes the session in a fresh one, from the branch it pushed."],
    [<Users />, "Bring your own keys", "A team API key per harness, or each person’s own. Encrypted at rest with a master key only you hold."],
  ];
  return (
    <Section id="self-host">
      <div className="grid gap-12 lg:grid-cols-2">
        <div className="min-w-0">
          <SectionHead eyebrow="Self-hosted" title="One Helm chart. Runs where your code does.">
            Hyperbeam is open source under the Apache 2.0 license. Try it on a laptop in a few minutes, then install it on Kubernetes for the team.
          </SectionHead>
          <CodeBlock code={HELM} lang="bash" title="Install" />
          <div className="mt-5 flex flex-wrap gap-3">
            <ButtonLink href={docs("install-kubernetes")} variant="secondary">Install on Kubernetes <ArrowRight /></ButtonLink>
            <ButtonLink href={docs("quickstart")} variant="ghost">Or try it locally</ButtonLink>
          </div>
        </div>
        <ul className="grid content-start gap-3 sm:grid-cols-2 lg:mt-24">
          {points.map(([icon, title, body]) => (
            <li key={title} className="grid content-start gap-2 rounded-xl border border-border bg-surface p-5">
              <div className="text-primary [&_svg]:size-5">{icon}</div>
              <h3 className="font-display text-[15px] font-semibold">{title}</h3>
              <p className="text-[13.5px] leading-relaxed text-muted">{body}</p>
            </li>
          ))}
        </ul>
      </div>
    </Section>
  );
}

function Status() {
  return (
    <Section className="pt-0 sm:pt-0">
      <div className="grid gap-6 rounded-xl border border-border bg-surface p-6 sm:p-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.3fr)]">
        <div>
          <div className="eyebrow mb-2">Tested</div>
          <h2 className="font-display text-[24px] font-semibold tracking-[-0.02em]">Tested end to end</h2>
          <p className="mt-2 text-[14.5px] leading-relaxed text-muted">The control plane, runner and CLI are covered by end-to-end tests against a real server, real runners and a real git backend, and the Helm chart is exercised on Kubernetes.</p>
          <a href={docs("status")} className="mt-3 inline-flex items-center gap-1.5 text-[14px] text-primary hover:underline">Testing and limits <ArrowRight className="size-3.5" /></a>
        </div>
        <ul className="grid content-start gap-2.5 text-[14px]">
          {[
            "Permissions, sign-up, invites and roles",
            "The full session lifecycle through the runner and ACP",
            "Cloning and pushing through the git proxy, with default-branch protection",
            "Opening pull requests through the gateway; denial and audit",
          ].map((t) => <li key={t} className="flex gap-2.5"><CircleCheck className="mt-0.5 size-4 shrink-0 text-run" />{t}</li>)}
        </ul>
      </div>
    </Section>
  );
}

function FinalCta() {
  return (
    <section className="relative overflow-hidden border-t border-border px-4 py-24 sm:px-8">
      <div aria-hidden="true" className="dot-grid pointer-events-none absolute inset-0" />
      <div className="relative mx-auto grid max-w-[640px] justify-items-center gap-5 text-center">
        <BrandMark size={48} />
        <p className="font-display text-[15px] font-semibold tracking-[-0.01em] text-primary">Because it’s the best move.</p>
        <h2 className="font-display text-[32px] font-semibold leading-tight tracking-[-0.03em] sm:text-[42px]">Give your agents a place the team can see.</h2>
        <p className="text-[16px] text-muted">Run it on your laptop and connect your first agent in a few minutes.</p>
        <div className="flex flex-wrap justify-center gap-3">
          <ButtonLink href={docs("quickstart")}>Get started <ArrowRight /></ButtonLink>
          {REPO_URL && <ButtonLink href={REPO_URL} variant="secondary"><GitHubIcon /> View the source</ButtonLink>}
        </div>
      </div>
    </section>
  );
}

function Footer() {
  const cols: [string, [string, string][]][] = [
    ["Product", [["How it works", "#how"], ["Security", "#security"], ["Self-host", "#self-host"]]],
    ["Docs", [["Quick start", docs("quickstart")], ["Concepts", docs("concepts")], ["Install on Kubernetes", docs("install-kubernetes")], ["Command line", docs("cli")]]],
    ["Project", [["Testing and limits", docs("status")], ["Development", docs("development")], ...(REPO_URL ? [["Source", REPO_URL] as [string, string]] : [])]],
  ];
  return (
    <footer className="border-t border-border px-4 py-12 sm:px-8">
      <div className="mx-auto grid max-w-[1160px] gap-10 md:grid-cols-[1.4fr_repeat(3,1fr)]">
        <div className="grid content-start gap-3">
          <a href="/" className="flex items-center gap-2"><BrandMark size={22} /><Wordmark className="text-[17px]" /></a>
          <p className="font-display text-[14px] font-semibold text-primary">Because it’s the best move.</p>
          <p className="max-w-[34ch] text-[13.5px] text-muted">A self-hosted platform where a team’s coding agents work in the open.</p>
          <p className="text-[12.5px] text-faint">Apache License 2.0</p>
        </div>
        {cols.map(([title, links]) => (
          <div key={title}>
            <div className="mb-3 text-[12px] font-semibold uppercase tracking-[0.07em] text-faint">{title}</div>
            <ul className="grid gap-2 text-[14px]">
              {links.map(([label, href]) => <li key={label}><a href={href} className="text-muted hover:text-foreground">{label}</a></li>)}
            </ul>
          </div>
        ))}
      </div>
    </footer>
  );
}

export function Page() {
  return (
    <>
      <a href="#main" className="sr-only focus:not-sr-only focus:fixed focus:left-3 focus:top-3 focus:z-50 focus:rounded-md focus:bg-surface focus:px-3 focus:py-2">Skip to content</a>
      <Nav />
      <main id="main">
        <Hero />
        <Harnesses />
        <HowItWorks />
        <Features />
        <Security />
        <Repeatable />
        <SelfHost />
        <Status />
        <FinalCta />
      </main>
      <Footer />
    </>
  );
}
