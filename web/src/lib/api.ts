// Thin client for the Hyperbeam HTTP API. The browser session cookie
// authenticates every call.

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

async function request<T = any>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api/v1${path}`, {
    method,
    headers: body === undefined ? undefined : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: "same-origin",
  });
  let data: any = null;
  try {
    data = await res.json();
  } catch {
    // Non-JSON error bodies fall through to the generic message below.
  }
  if (!res.ok) {
    throw new ApiError(res.status, data?.error ?? `The server answered ${res.status}.`);
  }
  return data as T;
}

export const api = {
  get: <T = any>(path: string) => request<T>("GET", path),
  post: <T = any>(path: string, body: unknown = {}) => request<T>("POST", path, body),
  put: <T = any>(path: string, body: unknown = {}) => request<T>("PUT", path, body),
  patch: <T = any>(path: string, body: unknown = {}) => request<T>("PATCH", path, body),
  del: <T = any>(path: string) => request<T>("DELETE", path),
};

export type Role = "owner" | "admin" | "member";

export interface User {
  id: string;
  email: string;
  name: string;
  role: Role;
  grants: string[];
  repo_scope: string[] | null;
  disabled: boolean;
  created_at: string;
}

export interface Link {
  kind: string;
  title: string;
  url: string;
}

export type SessionStatus = "pending" | "starting" | "running" | "idle" | "completed" | "failed" | "cancelled";

/** A question an agent asked with `hyperbeam_ask_user` that nobody has answered yet. */
export interface Question {
  id: string;
  text: string;
  /** Answers the agent suggested; people may always answer in their own words. */
  options: string[];
  /** A worker's question goes to its conductor first. */
  to: "team" | "conductor";
  asked_at: string;
}

export interface Session {
  id: string;
  workstream_id: string | null;
  parent_id: string | null;
  title: string;
  role: "worker" | "conductor";
  status: SessionStatus;
  harness_id: string;
  created_by: string;
  workflow: string | null;
  repos: string[];
  skills: string[];
  grants: string[];
  work_branch: string;
  model: string | null;
  links: Link[];
  headline: string;
  summary: string;
  error: string | null;
  /** Set while the agent is waiting on an answer, even after its sandbox has shut down. */
  question: Question | null;
  launches: number;
  created_at: string;
  started_at: string | null;
  ended_at: string | null;
  last_activity_at: string;
}

export type WorkstreamStatus = "scoping" | "building" | "review" | "done" | "archived";

export interface Workstream {
  id: string;
  title: string;
  description: string;
  status: WorkstreamStatus;
  repos: string[];
  links: Link[];
  created_by: string;
  created_at: string;
  updated_at: string;
}

export interface SessionEvent {
  session_id: string;
  seq: number;
  ts: string;
  kind: string;
  data: any;
}

export interface Activity {
  id: number;
  ts: string;
  user_id: string | null;
  session_id: string | null;
  workstream_id: string | null;
  kind: string;
  summary: string;
  data: any;
}

export interface GrantInfo {
  id: string;
  label: string;
  system: string;
  /** How to write the system's name: "GitHub", "Acme tools". */
  system_title: string;
}

export interface Meta {
  version: string;
  needs_setup: boolean;
  team_name: string | null;
  sandbox_driver: string;
}

export type ToolLevel = "read" | "write" | "hidden";

export interface ConnectionTool {
  name: string;
  title: string | null;
  description: string;
  annotations: { readOnlyHint?: boolean; destructiveHint?: boolean } | null;
  /** What agents may use it at. */
  level: ToolLevel;
  /** approved: an admin decided; automatic: the preset sorted it; pending: nobody has; changed: it changed since an admin decided. */
  status: "approved" | "automatic" | "pending" | "changed";
  suggested: ToolLevel | null;
}

export interface Connection {
  name: string;
  preset: string | null;
  title: string;
  url: string;
  /** `oauth` is set when the credential comes from an OAuth sign-in; `client_id` only when an admin supplied the client. */
  auth: { header: string | null; prefix: string; oauth: { client_id: string | null; scope: string | null } | null };
  headers: Record<string, string>;
  options: Record<string, string>;
  tools: ConnectionTool[];
  awaiting_review: number;
  tools_error: string | null;
  tools_refreshed_at: string | null;
  updated_at: string;
}

export interface PresetOption {
  key: string;
  label: string;
  placeholder?: string;
  hint?: string;
  choices?: { value: string; label: string }[];
  default?: string;
}

export interface Preset {
  id: string;
  title: string;
  blurb: string;
  secret_label: string;
  secret_help: string;
  /** key: an admin pastes a credential; oauth: an admin signs in with the vendor. */
  auth: "key" | "oauth";
  /** The admin creates an OAuth client with the vendor and enters its ID and secret. */
  oauth_client: boolean;
  oauth_client_help: string;
  options: PresetOption[];
  /** vendor: sorted by the vendor's read-only server; name: by tool name. */
  classified_by: "vendor" | "name";
  connected: boolean;
}

export interface CredentialSlot {
  env: string;
  label: string;
  kind: "api_key" | "oauth";
}

export interface ModelChoice {
  id: string;
  label: string;
  description: string;
}

export const modelLabel = (choices: ModelChoice[], id: string | null | undefined) =>
  id ? choices.find((m) => m.id === id)?.label || id : null;

export interface Harness {
  id: string;
  name: string;
  title: string;
  description: string;
  image: string;
  command: string[];
  env: Record<string, string>;
  credentials: CredentialSlot[];
  egress_hosts: string[];
  builtin: boolean;
  /** Admin-curated list; empty means "whatever the harness offers". */
  models: ModelChoice[];
  discovered_models: ModelChoice[];
  default_model: string | null;
  model_env: string | null;
  /** What to offer when starting work. */
  model_choices: ModelChoice[];
  /** Hyperbeam is asking the harness which models it offers. */
  models_refreshing: boolean;
  /** Why the last attempt to ask failed, if it did. */
  models_error: string | null;
  team_credential: string | null;
  my_credential: string | null;
  ready: boolean;
}

/** Where repositories live. Each has its own integration and permissions. */
export type CodeHost = "github" | "gitlab";

export type ChatPlatform = "slack" | "discord";

/** A Slack or Discord bot people start and direct agents from. */
export interface ChatApp {
  platform: ChatPlatform;
  connected: boolean;
  config: { auto_link_email?: boolean; workspace?: string; bot_name?: string };
  /** How the connection to the platform is doing right now. */
  status?: { state: "connecting" | "connected" | "error"; detail: string } | null;
  updated_at?: string;
}

export interface ChatChannel {
  id: string;
  name: string;
  private: boolean;
  /** Shared with another organisation; agents cannot be started in it. */
  external: boolean;
  enabled: boolean;
  /** Enabled once, but the bot is no longer in it. */
  missing?: boolean;
  repos: string[];
  harness: string | null;
  role: "worker" | "conductor";
}

/** A chat account that acts as a person. */
export interface ChatIdentity {
  platform: ChatPlatform;
  external_id: string;
  display_name: string;
  linked_by: "email" | "code";
  linked_at: string;
}

/** Something an agent you started wants you to know. */
export interface Notification {
  id: string;
  session_id: string;
  /** It asked you a question, finished a turn and is waiting, or failed. */
  kind: "question" | "finished" | "failed";
  /** The agent's title when it happened. */
  title: string;
  /** The question, the agent's closing message, or the error. */
  body: string;
  created_at: string;
  read_at: string | null;
}

/** A chat app that can also send you your notifications, as a direct message. */
export interface NotificationChannel {
  platform: ChatPlatform;
  enabled: boolean;
  /** An admin has connected the bot. */
  connected: boolean;
  /** Your linked accounts there, by display name. */
  accounts: string[];
}

export interface Repo {
  id: string;
  host: CodeHost;
  full_name: string;
  default_branch: string;
  description: string;
  accessible: boolean;
}

export interface Skill {
  id: string;
  name: string;
  description: string;
  content: string;
  updated_at: string;
}

export interface WorkflowInput {
  name: string;
  description: string;
  required: boolean;
  default: string | null;
}

export interface Workflow {
  id: string;
  name: string;
  description: string;
  yaml: string;
  updated_at: string;
  spec: {
    name: string;
    description: string;
    role: "worker" | "conductor";
    harness: string | null;
    model: string | null;
    repos: string[];
    skills: string[];
    grants: string[];
    inputs: WorkflowInput[];
    prompt: string;
  } | null;
}

export const LIVE: SessionStatus[] = ["pending", "starting", "running", "idle"];
export const isLive = (s: Session) => LIVE.includes(s.status);
