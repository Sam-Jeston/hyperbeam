// The cast and script of the landing page's egress scene: agents started by people, the three ways out of a
// sandbox, and a loop of requests Hyperbeam lets through or refuses. Illustrative, not a recording.
// Shared by page.tsx (the static render) and egress.ts (the animation).

export type Lane = "git" | "tools" | "egress";

export const PEOPLE = {
  PK: { name: "Priya", hue: 282 },
  JM: { name: "Jordan", hue: 160 },
  AL: { name: "Alex", hue: 30 },
} as const;
export type Person = keyof typeof PEOPLE;

export const AGENTS = [
  { id: "claude", harness: "Claude Code", repo: "acme/api", owner: "PK", via: null, grants: ["github:write", "github:open_pr"] },
  { id: "codex", harness: "Codex", repo: "acme/web", owner: "PK", via: "worker of Claude Code", grants: ["github:write"] },
  { id: "gemini", harness: "Gemini CLI", repo: "acme/web", owner: "JM", via: null, grants: ["github:read", "linear:write"] },
  { id: "opencode", harness: "OpenCode", repo: "acme/docs", owner: "AL", via: null, grants: ["notion:write"] },
] as const satisfies readonly { id: string; harness: string; repo: string; owner: Person; via: string | null; grants: readonly string[] }[];
export type AgentId = (typeof AGENTS)[number]["id"];

export const LANES: { id: Lane; label: string; hint: string }[] = [
  { id: "git", label: "Git proxy", hint: "clone · push" },
  { id: "tools", label: "Tool gateway", hint: "every call, every grant" },
  { id: "egress", label: "Egress allowlist", hint: "everything else" },
];

export const DESTS = [
  { id: "github", label: "GitHub", allowed: true },
  { id: "linear", label: "Linear", allowed: true },
  { id: "notion", label: "Notion", allowed: true },
  { id: "npm", label: "registry.npmjs.org", allowed: true },
  { id: "pastebin", label: "pastebin.com", allowed: false },
  { id: "metadata", label: "169.254.169.254", allowed: false },
] as const;
export type DestId = (typeof DESTS)[number]["id"];

export type Request = { kind: "request"; agent: AgentId; lane: Lane; dest: DestId; action: string; ok: boolean; reason: string };
export type GrantChange = { kind: "revoke" | "restore"; owner: Person; grant: string; text: string };
export type Beat = Request | GrantChange;

const req = (agent: AgentId, lane: Lane, dest: DestId, action: string, ok: boolean, reason: string): Request =>
  ({ kind: "request", agent, lane, dest, action, ok, reason });

/** One loop of the scene. Each beat plays about a second and a half after the last. */
export const BEATS: Beat[] = [
  req("claude", "git", "github", "push hyperbeam/audit-csv-3ff6c49b", true, "github:write"),
  req("opencode", "egress", "pastebin", "POST pastebin.com", false, "not on the allowlist"),
  req("codex", "egress", "npm", "npm install", true, "allowlisted"),
  req("gemini", "git", "github", "push hyperbeam/fix-nav-0c7d2e19", false, "Jordan can’t write to acme/web"),
  req("gemini", "tools", "linear", "linear: create issue", true, "linear:write"),
  req("claude", "egress", "metadata", "GET 169.254.169.254", false, "not on the allowlist"),
  req("opencode", "tools", "notion", "notion: append page", true, "notion:write"),
  req("codex", "tools", "github", "github: open pull request", false, "worker holds less than its conductor"),
  req("claude", "tools", "github", "github: open acme/api#418", true, "github:open_pr"),
  { kind: "revoke", owner: "PK", grant: "github:write", text: "Admin removed github:write from Priya" },
  req("codex", "git", "github", "push hyperbeam/csv-ui-5b8e44a2", false, "Priya no longer holds github:write"),
  req("claude", "git", "github", "push hyperbeam/audit-csv-3ff6c49b", false, "Priya no longer holds github:write"),
  req("gemini", "egress", "npm", "npm install", true, "allowlisted"),
  { kind: "restore", owner: "PK", grant: "github:write", text: "Admin gave github:write back to Priya" },
];

/** Background traffic: routine requests that are always allowed, drawn small and unlabelled. */
export const AMBIENT: { agent: AgentId; lane: Lane; dest: DestId }[] = [
  { agent: "claude", lane: "git", dest: "github" },
  { agent: "codex", lane: "git", dest: "github" },
  { agent: "gemini", lane: "git", dest: "github" },
  { agent: "claude", lane: "egress", dest: "npm" },
  { agent: "codex", lane: "egress", dest: "npm" },
  { agent: "gemini", lane: "egress", dest: "npm" },
  { agent: "opencode", lane: "egress", dest: "npm" },
  { agent: "gemini", lane: "tools", dest: "linear" },
  { agent: "opencode", lane: "tools", dest: "notion" },
];

const agentOf = (id: AgentId) => AGENTS.find((a) => a.id === id)!;

/** One audit log row as HTML, so the static page and the live scene render it identically. */
export function logRow(beat: Beat, time: string): string {
  if (beat.kind !== "request") {
    const tag = beat.kind === "revoke" ? `<span class="elog-tag bg-wait/14 text-wait">Revoked</span>` : `<span class="elog-tag bg-idle/14 text-idle">Granted</span>`;
    return `<li class="elog-row border-border/60"><span class="elog-time">${time}</span>${tag}<span class="elog-what">${beat.text}</span><span class="elog-why">applies to running agents now</span></li>`;
  }
  const agent = agentOf(beat.agent);
  const person = PEOPLE[agent.owner];
  const tag = beat.ok ? `<span class="elog-tag bg-run/14 text-run">Allowed</span>` : `<span class="elog-tag bg-fail/14 text-fail">Blocked</span>`;
  return `<li class="elog-row border-border/60"><span class="elog-time">${time}</span>${tag}<span class="elog-what"><b>${agent.harness}</b> for ${person.name} · <code>${beat.action}</code></span><span class="elog-why">${beat.reason}</span></li>`;
}

/** A plausible clock for the log, starting mid-afternoon. */
export function clock(seconds: number): string {
  const t = 14 * 3600 + 2 * 60 + 7 + Math.floor(seconds);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(Math.floor(t / 3600) % 24)}:${p(Math.floor(t / 60) % 60)}:${p(t % 60)}`;
}
