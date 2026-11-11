// Team-wide live state. One fetch on load, then kept current by the
// server's event stream, so every screen shows what is happening now.

import { useSyncExternalStore } from "react";
import { api, type Activity, type Meta, type Session, type User, type Workstream } from "./api";

export interface State {
  meta: Meta | null;
  me: User | null;
  /** null until the first load finishes. */
  ready: boolean;
  users: Record<string, User>;
  sessions: Record<string, Session>;
  workstreams: Record<string, Workstream>;
  activity: Activity[];
  connected: boolean;
}

let state: State = {
  meta: null,
  me: null,
  ready: false,
  users: {},
  sessions: {},
  workstreams: {},
  activity: [],
  connected: false,
};

const listeners = new Set<() => void>();

function set(patch: Partial<State>) {
  state = { ...state, ...patch };
  listeners.forEach((fn) => fn());
}

export function useStore<T>(select: (s: State) => T): T {
  return useSyncExternalStore(
    (fn) => {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
    () => select(state),
  );
}

export const getState = () => state;

const byId = <T extends { id: string }>(items: T[]) => Object.fromEntries(items.map((i) => [i.id, i]));

let stream: EventSource | null = null;

export async function boot() {
  const meta = await api.get<Meta>("/meta");
  let me: User | null = null;
  if (!meta.needs_setup) {
    try {
      me = (await api.get("/me")).user;
    } catch {
      me = null;
    }
  }
  set({ meta, me, ready: !me });
  if (me) await loadTeam();
}

export async function signedIn(user: User) {
  set({ me: user, meta: state.meta ? { ...state.meta, needs_setup: false } : state.meta });
  const meta = await api.get<Meta>("/meta");
  set({ meta });
  await loadTeam();
}

export async function signOut() {
  await api.post("/auth/logout").catch(() => {});
  stream?.close();
  stream = null;
  set({ me: null, users: {}, sessions: {}, workstreams: {}, activity: [], connected: false });
}

export async function refreshMe() {
  const { user } = await api.get("/me");
  set({ me: user });
}

async function loadTeam() {
  const [users, sessions, workstreams, activity] = await Promise.all([
    api.get("/users"),
    api.get("/sessions?limit=500"),
    api.get("/workstreams?include_archived=true"),
    api.get("/activity?limit=100"),
  ]);
  set({
    users: byId<User>(users.users),
    sessions: byId<Session>(sessions.sessions),
    workstreams: byId<Workstream>(workstreams.workstreams),
    activity: activity.activity,
    ready: true,
  });
  connect();
}

export async function reloadUsers() {
  const users = await api.get("/users");
  set({ users: byId<User>(users.users) });
}

function connect() {
  stream?.close();
  const es = new EventSource("/api/v1/stream");
  stream = es;
  es.onopen = () => set({ connected: true });
  // EventSource reconnects by itself; refetch on return in case we missed anything.
  es.onerror = () => {
    if (state.connected) set({ connected: false });
  };
  let wasDown = false;
  es.addEventListener("open", () => {
    if (wasDown) void loadTeam();
    wasDown = false;
  });
  es.addEventListener("error", () => {
    wasDown = true;
  });
  es.addEventListener("session", (e) => {
    const patch = JSON.parse((e as MessageEvent).data);
    const current = state.sessions[patch.id];
    if (!current && !patch.title) {
      // A partial update for a session we have not seen: fetch it whole.
      void api.get(`/sessions/${patch.id}`).then((r) => set({ sessions: { ...state.sessions, [patch.id]: r.session } })).catch(() => {});
      return;
    }
    set({ sessions: { ...state.sessions, [patch.id]: { ...current, ...patch } } });
  });
  es.addEventListener("workstream", (e) => {
    const patch = JSON.parse((e as MessageEvent).data);
    if (patch.title) {
      set({ workstreams: { ...state.workstreams, [patch.id]: patch } });
    } else {
      void api.get(`/workstreams/${patch.id}`).then((r) => set({ workstreams: { ...state.workstreams, [patch.id]: r.workstream } })).catch(() => {});
    }
    window.dispatchEvent(new CustomEvent("noemune:workstream", { detail: patch.id }));
  });
  es.addEventListener("activity", (e) => {
    const item = JSON.parse((e as MessageEvent).data);
    set({ activity: [item, ...state.activity].slice(0, 300) });
  });
  es.addEventListener("resync", () => void loadTeam());
}
