import { useState } from "react";
import { isLive } from "../lib/api";
import { useStore } from "../lib/store";
import { Empty, useTick } from "../lib/ui";
import { AgentRow, Feed } from "./Board";

export function AgentsPage() {
  useTick();
  const sessions = useStore((s) => s.sessions);
  const me = useStore((s) => s.me)!;
  const [filter, setFilter] = useState<"live" | "mine" | "all">("live");
  const list = Object.values(sessions)
    .filter((s) => (filter === "live" ? isLive(s) : filter === "mine" ? s.created_by === me.id : true))
    .sort((a, b) => b.last_activity_at.localeCompare(a.last_activity_at));
  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Agents</h1>
          <p>Every agent session on the team. Open one to watch it work.</p>
        </div>
      </div>
      <div className="tabs">
        <button className={filter === "live" ? "active" : ""} onClick={() => setFilter("live")}>Live now</button>
        <button className={filter === "mine" ? "active" : ""} onClick={() => setFilter("mine")}>Started by me</button>
        <button className={filter === "all" ? "active" : ""} onClick={() => setFilter("all")}>All</button>
      </div>
      {list.length === 0 ? (
        <Empty title={filter === "live" ? "No agents are running" : "No agents yet"}>Start one from the board with “New work”.</Empty>
      ) : (
        <div className="card">{list.map((s) => <AgentRow key={s.id} session={s} />)}</div>
      )}
    </div>
  );
}

export function ActivityPage() {
  useTick();
  const activity = useStore((s) => s.activity);
  return (
    <div className="page" style={{ maxWidth: 760 }}>
      <div className="page-head">
        <div>
          <h1>Activity</h1>
          <p>What people and agents have done recently.</p>
        </div>
      </div>
      <div className="card card-pad"><Feed items={activity} /></div>
    </div>
  );
}
