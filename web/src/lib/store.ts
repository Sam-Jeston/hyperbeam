// Team-wide live state. One fetch on load, then kept current by the
// server's event stream, so every screen shows what is happening now.

import { useSyncExternalStore } from "react";
import { api, type Activity, type GrantInfo, type Meta, type Notification, type Session, type User, type Workstream } from "./api";

export interface State {
  meta: Meta | null;
  me: User | null;
  /** Every permission that can be granted, including each connection's. */
  grants: GrantInfo[];
  /** null until the first load finishes. */
  ready: boolean;
  users: Record<string, User>;
  sessions: Record<string, Session>;
  workstreams: Record<string, Workstream>;
  activity: Activity[];
  /** The signed-in person's own, newest first. */
  notifications: Notification[];
  /** How many they have not read, including any older than the list holds. */
  unread: number;
  connected: boolean;
}

let state: State = {
  meta: null,
  me: null,
  grants: [],
  ready: false,
  users: {},
  sessions: {},
  workstreams: {},
  activity: [],
  notifications: [],
  unread: 0,
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
  set({ me: null, users: {}, sessions: {}, workstreams: {}, activity: [], notifications: [], unread: 0, connected: false });
}

export async function refreshMe() {
  const { user } = await api.get("/me");
  set({ me: user });
}

async function loadTeam() {
  const [grants, users, sessions, workstreams, activity, notifications] = await Promise.all([
    api.get("/grants"),
    api.get("/users"),
    api.get("/sessions?limit=500"),
    api.get("/workstreams?include_archived=true"),
    api.get("/activity?limit=100"),
    api.get("/notifications"),
  ]);
  set({
    grants: grants.grants,
    users: byId<User>(users.users),
    sessions: byId<Session>(sessions.sessions),
    workstreams: byId<Workstream>(workstreams.workstreams),
    activity: activity.activity,
    notifications: notifications.notifications,
    unread: notifications.unread,
    ready: true,
  });
  connect();
}

/** After connections change: what can be granted, and who holds what. */
export async function reloadGrants() {
  const [grants, users] = await Promise.all([api.get("/grants"), api.get("/users")]);
  set({ grants: grants.grants, users: byId<User>(users.users) });
}

export async function reloadUsers() {
  const users = await api.get("/users");
  set({ users: byId<User>(users.users) });
}

async function loadNotifications() {
  const { notifications, unread } = await api.get("/notifications");
  set({ notifications, unread });
}

/**
 * Mark notifications as read: one, those about one session, or all of them.
 * Shown at once; the server then tells this person's other tabs.
 */
export function markRead(which: { id?: string; session_id?: string } = {}) {
  const hit = (n: Notification) => !n.read_at && (!which.id || n.id === which.id) && (!which.session_id || n.session_id === which.session_id);
  const targeted = which.id || which.session_id;
  const shown = state.notifications.filter(hit).length;
  // Nothing listed to mark, and nothing unread that the list is too short to hold.
  if (shown === 0 && state.unread <= state.notifications.filter((n) => !n.read_at).length) return;
  const now = new Date().toISOString();
  set({
    notifications: state.notifications.map((n) => (hit(n) ? { ...n, read_at: now } : n)),
    unread: targeted ? Math.max(0, state.unread - shown) : 0,
  });
  // On success the server's `notifications` event brings every tab into line.
  void api.post("/notifications/read", which).catch(() => loadNotifications().catch(() => {}));
}

/** Is this person looking at that agent's page right now? */
const watching = (sessionId: string) =>
  document.visibilityState === "visible" && [`/s/${sessionId}`, `/sessions/${sessionId}`].includes(window.location.pathname);

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
    if (patch.deleted) {
      // Opened for work that then could not start.
      const { [patch.id as string]: _gone, ...rest } = state.workstreams;
      set({ workstreams: rest });
    } else if (patch.title) {
      set({ workstreams: { ...state.workstreams, [patch.id]: patch } });
    } else {
      void api.get(`/workstreams/${patch.id}`).then((r) => set({ workstreams: { ...state.workstreams, [patch.id]: r.workstream } })).catch(() => {});
    }
    window.dispatchEvent(new CustomEvent("hyperbeam:workstream", { detail: patch.id }));
  });
  es.addEventListener("activity", (e) => {
    const item = JSON.parse((e as MessageEvent).data);
    set({ activity: [item, ...state.activity].slice(0, 300) });
  });
  es.addEventListener("notification", (e) => {
    const item: Notification = JSON.parse((e as MessageEvent).data);
    if (state.notifications.some((n) => n.id === item.id)) return;
    set({ notifications: [item, ...state.notifications].slice(0, 300), unread: state.unread + 1 });
    // They are watching it happen: there is nothing to catch up on.
    if (watching(item.session_id)) markRead({ id: item.id });
  });
  // Read somewhere else, perhaps in another tab.
  es.addEventListener("notifications", () => void loadNotifications().catch(() => {}));
  es.addEventListener("resync", () => void loadTeam());
}
