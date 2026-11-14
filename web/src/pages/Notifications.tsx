// The notification hub: what your agents wanted you to know, and the
// settings for being told in Slack or Discord as well.

import { Bell, CheckCheck, CircleAlert, CircleCheck, MessageCircleQuestion, Settings } from "lucide-react";
import { type ReactNode, useState } from "react";
import { Button } from "../components/ui/button";
import { Switch } from "../components/ui/controls";
import { TabCount, Tabs } from "../components/ui/menu";
import { Card, EmptyState } from "../components/ui/surface";
import { api, type Notification, type NotificationChannel } from "../lib/api";
import { markRead, useStore } from "../lib/store";
import { A, ErrorNote, navigate, Page, PageHeader, Time, useAction, useDocumentTitle, useLoad, useTick } from "../lib/ui";
import { cn } from "../lib/utils";
import { CHAT_TITLE } from "./Chat";

const KIND: Record<Notification["kind"], { icon: ReactNode; tone: string; says: string }> = {
  question: { icon: <MessageCircleQuestion />, tone: "text-wait", says: "has a question for you" },
  finished: { icon: <CircleCheck />, tone: "text-run", says: "finished and is waiting for you" },
  failed: { icon: <CircleAlert />, tone: "text-fail", says: "failed" },
};

/** Markdown as one plain run of text, for a preview. */
const plain = (text: string) => text.replace(/\*\*|`/g, "").replace(/^#+\s*/gm, "").replace(/\s+/g, " ").trim();

function Row({ item }: { item: Notification }) {
  // The agent may have been renamed since; show what it is called now.
  const title = useStore((s) => s.sessions[item.session_id]?.title) ?? item.title;
  const kind = KIND[item.kind];
  const unread = !item.read_at;
  const body = plain(item.body);
  return (
    <A
      href={`/s/${item.session_id}`}
      className="flex items-start gap-3 px-4 py-3 transition-colors hover:bg-surface-2/70 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring"
    >
      <span className={cn("mt-0.5 shrink-0 [&_svg]:size-4", unread ? kind.tone : "text-faint")}>{kind.icon}</span>
      <div className="grid min-w-0 flex-1 gap-0.5">
        <div className={cn("text-[13.5px]", unread ? "text-foreground" : "text-muted")}>
          <span className={unread ? "font-semibold" : "font-medium"}>{title}</span> {kind.says}
        </div>
        {body && <div className="line-clamp-2 text-[12.5px] text-muted">{body}</div>}
      </div>
      <Time iso={item.created_at} className="shrink-0 pt-0.5 text-[12px] text-faint" />
      <span className={cn("mt-1.5 size-2 shrink-0 rounded-full", unread ? "bg-primary" : "bg-transparent")} aria-label={unread ? "Unread" : undefined} />
    </A>
  );
}

export function NotificationsPage() {
  useTick();
  const notifications = useStore((s) => s.notifications);
  const unread = useStore((s) => s.unread);
  useDocumentTitle(unread ? `Notifications (${unread})` : "Notifications");
  const [filter, setFilter] = useState<"all" | "unread">("all");
  const list = filter === "unread" ? notifications.filter((n) => !n.read_at) : notifications;
  return (
    <Page width="narrow">
      <PageHeader
        title="Notifications"
        description="When an agent you started asks you a question, finishes a turn or fails, it is listed here."
        actions={
          <>
            <Button variant="ghost" onClick={() => navigate("/settings/account")}><Settings /> Settings</Button>
            <Button variant="secondary" disabled={unread === 0} onClick={() => markRead()}><CheckCheck /> Mark all read</Button>
          </>
        }
      />
      <Tabs
        className="mb-4"
        value={filter}
        onValueChange={setFilter}
        tabs={[
          { value: "all", label: "All" },
          { value: "unread", label: <>Unread <TabCount n={unread} /></> },
        ]}
      />
      {list.length === 0 ? (
        <EmptyState icon={<Bell />} title={filter === "unread" ? "You’re all caught up" : "Nothing yet"}>
          {filter === "unread"
            ? "Nothing is waiting on you."
            : "Start an agent and you will hear from it here when it needs you. You can also be told in Slack or Discord, under Settings."}
        </EmptyState>
      ) : (
        <Card className="divide-y divide-border overflow-hidden">{list.map((n) => <Row key={n.id} item={n} />)}</Card>
      )}
    </Page>
  );
}

/** Per person: which chat apps also send them their notifications. */
export function NotificationChannels() {
  const { data, error, reload } = useLoad<{ channels: NotificationChannel[] }>(() => api.get("/me/notification-channels"), []);
  const action = useAction();
  const toggle = (channel: NotificationChannel, enabled: boolean) =>
    void action
      .run(() => api.put(`/me/notification-channels/${channel.platform}`, { enabled }), {
        success: enabled ? `${CHAT_TITLE[channel.platform]} will message you` : `${CHAT_TITLE[channel.platform]} notifications turned off`,
        toastError: true,
      })
      .then(reload);
  return (
    <>
      <ErrorNote error={error} />
      {data && (
        <Card className="divide-y divide-border">
          {data.channels.map((channel) => {
            const title = CHAT_TITLE[channel.platform];
            const ready = channel.connected && channel.accounts.length > 0;
            const id = `notify-${channel.platform}`;
            return (
              <div key={channel.platform} className="flex items-center justify-between gap-4 px-3.5 py-3">
                <label htmlFor={id} className="grid gap-0.5 text-[13.5px]">
                  <span className="font-medium">Direct message on {title}</span>
                  <span className="text-[12.5px] text-muted">
                    {!channel.connected
                      ? `${title} is not connected. A team admin can connect it under Settings → Chat apps.`
                      : channel.accounts.length === 0
                        ? `Your ${title} account is not linked yet. Mention the Hyperbeam bot in ${title} and it will link you.`
                        : `The Hyperbeam bot messages ${channel.accounts.join(", ")}.`}
                  </span>
                </label>
                {/* Something already on can always be turned off. */}
                <Switch id={id} checked={channel.enabled} disabled={action.busy || (!ready && !channel.enabled)} onCheckedChange={(on) => toggle(channel, on)} />
              </div>
            );
          })}
        </Card>
      )}
      <p className="text-[12.5px] text-muted">An agent you started from a chat thread speaks in that thread, so it does not also message you there.</p>
    </>
  );
}
