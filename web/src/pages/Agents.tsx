import { Activity as ActivityIcon, Bot, Plus } from "lucide-react";
import { useState } from "react";
import { Button } from "../components/ui/button";
import { TabCount, Tabs } from "../components/ui/menu";
import { Card, EmptyState } from "../components/ui/surface";
import { isLive } from "../lib/api";
import { useStore } from "../lib/store";
import { openComposer, Page, PageHeader, useDocumentTitle, useTick } from "../lib/ui";
import { AgentRow, Feed } from "./Board";

export function AgentsPage() {
  useTick();
  useDocumentTitle("Agents");
  const sessions = useStore((s) => s.sessions);
  const me = useStore((s) => s.me)!;
  const [filter, setFilter] = useState<"live" | "mine" | "all">("live");
  const all = Object.values(sessions);
  const counts = { live: all.filter(isLive).length, mine: all.filter((s) => s.created_by === me.id).length, all: all.length };
  const list = all
    .filter((s) => (filter === "live" ? isLive(s) : filter === "mine" ? s.created_by === me.id : true))
    .sort((a, b) => b.last_activity_at.localeCompare(a.last_activity_at));
  return (
    <Page>
      <PageHeader
        title="Agents"
        description="Every agent session on the team. Open one to watch it work or to direct it."
        actions={<Button variant="primary" onClick={() => openComposer()}><Plus /> New work</Button>}
      />
      <Tabs
        className="mb-4"
        value={filter}
        onValueChange={setFilter}
        tabs={[
          { value: "live", label: <>Live now <TabCount n={counts.live} /></> },
          { value: "mine", label: <>Started by me <TabCount n={counts.mine} /></> },
          { value: "all", label: <>All <TabCount n={counts.all} /></> },
        ]}
      />
      {list.length === 0 ? (
        <EmptyState
          icon={<Bot />}
          title={filter === "live" ? "No agents are running" : "No agents yet"}
          action={<Button variant="primary" onClick={() => openComposer()}><Plus /> New work</Button>}
        >
          {filter === "live" ? "Agents that are working or waiting for a reply show up here." : "Agents you and your team start show up here."}
        </EmptyState>
      ) : (
        <Card className="divide-y divide-border overflow-hidden">{list.map((s) => <AgentRow key={s.id} session={s} />)}</Card>
      )}
    </Page>
  );
}

export function ActivityPage() {
  useTick();
  useDocumentTitle("Activity");
  const activity = useStore((s) => s.activity);
  return (
    <Page width="narrow">
      <PageHeader title="Activity" description="What people and agents have done recently." />
      {activity.length === 0 ? (
        <EmptyState icon={<ActivityIcon />} title="Nothing yet">When people start agents, comment, or open pull requests, it shows up here.</EmptyState>
      ) : (
        <Card className="px-5 py-4"><Feed items={activity} grouped /></Card>
      )}
    </Page>
  );
}
