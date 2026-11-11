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
}

export interface Meta {
  version: string;
  needs_setup: boolean;
  team_name: string | null;
  grants: GrantInfo[];
  sandbox_driver: string;
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

export interface Repo {
  id: string;
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
