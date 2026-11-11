import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { isLive } from "./lib/api";
import { boot, signOut, useStore } from "./lib/store";
import { A, Avatar, usePath } from "./lib/ui";
import { AuthScreen } from "./pages/Auth";
import { Board } from "./pages/Board";
import { WorkstreamPage } from "./pages/Workstream";
import { SessionPage } from "./pages/Session";
import { AgentsPage, ActivityPage } from "./pages/Agents";
import { LibraryPage } from "./pages/Library";
import { TeamPage } from "./pages/Team";
import { SettingsPage } from "./pages/Settings";

function Mark() {
  return (
    <span className="brand-mark" aria-hidden>
      <svg width="16" height="16" viewBox="0 0 16 16">
        <circle cx="5" cy="8" r="2.6" fill="white" />
        <circle cx="11.5" cy="4.2" r="1.9" fill="white" opacity=".8" />
        <circle cx="11.5" cy="11.8" r="1.9" fill="white" opacity=".8" />
      </svg>
    </span>
  );
}

function Shell() {
  const path = usePath();
  const me = useStore((s) => s.me)!;
  const teamName = useStore((s) => s.meta?.team_name);
  const connected = useStore((s) => s.connected);
  const liveCount = useStore((s) => Object.values(s.sessions).filter(isLive).length);

  const link = (href: string, label: string, badge?: number) => {
    const active = href === "/" ? path === "/" || path.startsWith("/w/") : path.startsWith(href);
    return (
      <A href={href} className={active ? "active" : ""}>
        {label}
        {badge ? <span className="count">{badge}</span> : null}
      </A>
    );
  };

  let page;
  const [, a, b] = path.split("/");
  if (a === "" || a === undefined) page = <Board />;
  else if (a === "w" && b) page = <WorkstreamPage key={b} id={b} />;
  else if (a === "s" && b) page = <SessionPage key={b} id={b} />;
  else if (a === "sessions" && b) page = <SessionPage key={b} id={b} />;
  else if (a === "agents") page = <AgentsPage />;
  else if (a === "activity") page = <ActivityPage />;
  else if (a === "library") page = <LibraryPage tab={b ?? "skills"} />;
  else if (a === "team") page = <TeamPage />;
  else if (a === "settings") page = <SettingsPage tab={b ?? "integrations"} />;
  else page = <div className="page"><h1>Nothing here</h1><p className="muted">That page does not exist. <A href="/">Back to the board</A></p></div>;

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand">
          <Mark />
          <div>
            Noemune
            <small>{teamName}</small>
          </div>
        </div>
        <nav className="nav">
          {link("/", "Board")}
          {link("/agents", "Agents", liveCount)}
          {link("/activity", "Activity")}
          <div className="nav-section">Build with</div>
          {link("/library", "Library")}
          <div className="nav-section">Team</div>
          {link("/team", "People & permissions")}
          {link("/settings", "Settings")}
        </nav>
        <div className="sidebar-foot">
          <Avatar user={me} size={26} />
          <div className="grow">
            <div className="truncate" style={{ fontWeight: 500 }}>{me.name}</div>
            <div className="small muted">{me.role}</div>
          </div>
          <button className="btn btn-sm btn-ghost" onClick={() => void signOut()}>Sign out</button>
        </div>
      </aside>
      <main className="main">
        {!connected && <div className="offline">Reconnecting to the server… what you see may be out of date.</div>}
        {page}
      </main>
    </div>
  );
}

function App() {
  const ready = useStore((s) => s.ready);
  const me = useStore((s) => s.me);
  const meta = useStore((s) => s.meta);
  const [bootError, setBootError] = useState<string | null>(null);

  useEffect(() => {
    boot().catch((e) => setBootError(e instanceof Error ? e.message : String(e)));
  }, []);

  if (bootError) {
    return (
      <div className="auth">
        <div className="card auth-card">
          <h1>Can’t reach Noemune</h1>
          <p className="muted">{bootError}</p>
          <button className="btn" onClick={() => window.location.reload()}>Try again</button>
        </div>
      </div>
    );
  }
  if (!meta || (me && !ready)) return null;
  if (!me) return <AuthScreen />;
  return <Shell />;
}

createRoot(document.getElementById("root")!).render(<App />);
