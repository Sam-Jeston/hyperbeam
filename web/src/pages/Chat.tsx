import { Hash, Link2, Lock, MessagesSquare, SlidersHorizontal } from "lucide-react";
import { useState } from "react";
import { BrandMark } from "../components/ui/brand";
import { Button } from "../components/ui/button";
import { Switch, SwitchRow, ToggleChip } from "../components/ui/controls";
import { confirm, Modal } from "../components/ui/dialog";
import { Field, Input, Textarea } from "../components/ui/input";
import { Select } from "../components/ui/select";
import { Badge, Card, EmptyState, Notice, Skeleton } from "../components/ui/surface";
import { api, type ChatApp, type ChatChannel, type ChatIdentity, type ChatPlatform, type Harness, type Repo } from "../lib/api";
import { useStore } from "../lib/store";
import { A, ErrorNote, navigate, Page, Time, useAction, useDocumentTitle, useLoad } from "../lib/ui";

export const CHAT_TITLE: Record<ChatPlatform, string> = { slack: "Slack", discord: "Discord" };

const COPY: Record<ChatPlatform, { blurb: string; tile: string }> = {
  slack: { blurb: "Mention the bot in a channel, or message it directly, to start an agent. The thread becomes the conversation with it.", tile: "S" },
  discord: { blurb: "Mention the bot in a channel to start an agent. It opens a thread, and the thread becomes the conversation with it.", tile: "D" },
};

/** What the Slack app needs, ready to paste into "Create an app from a manifest". */
const SLACK_MANIFEST = JSON.stringify(
  {
    display_information: { name: "Hyperbeam" },
    features: {
      app_home: { messages_tab_enabled: true, messages_tab_read_only_enabled: false },
      bot_user: { display_name: "Hyperbeam", always_online: true },
    },
    oauth_config: {
      scopes: {
        bot: ["channels:history", "channels:read", "chat:write", "groups:history", "groups:read", "im:history", "reactions:write", "users:read", "users:read.email"],
      },
    },
    settings: {
      event_subscriptions: { bot_events: ["message.channels", "message.groups", "message.im"] },
      interactivity: { is_enabled: true },
      socket_mode_enabled: true,
      org_deploy_enabled: false,
      token_rotation_enabled: false,
    },
  },
  null,
  2,
);

// ------------------------------------------------------------ settings tab

export function ChatApps({ isAdmin }: { isAdmin: boolean }) {
  const { data, error, reload } = useLoad<{ apps: ChatApp[] }>(() => api.get("/chat"), []);
  const [editing, setEditing] = useState<ChatApp | null>(null);
  return (
    <>
      <p className="mb-5 max-w-[72ch] text-[13.5px] text-muted">
        Start and direct agents from where your team already talks. Someone who writes to the bot acts as their Hyperbeam account, with their permissions, so each person’s chat account must be linked to it first. An agent’s replies are seen by everyone in the channel, so you choose which channels it may be started in.
      </p>
      <ErrorNote error={error} />
      {!data && !error && <div className="grid gap-3">{[0, 1].map((i) => <Skeleton key={i} className="h-24 rounded-lg" />)}</div>}
      <div className="grid gap-3">
        {data?.apps.map((app) => {
          const title = CHAT_TITLE[app.platform];
          const failed = app.status?.state === "error";
          return (
            <Card key={app.platform} className="grid gap-3 p-4">
              <div className="flex flex-wrap items-start gap-4">
                <div className="grid size-10 shrink-0 place-items-center rounded-lg bg-surface-2 font-display text-[17px] font-bold text-foreground">{COPY[app.platform].tile}</div>
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <h2 className="font-display text-[15.5px] font-semibold">{title}</h2>
                    {!app.connected ? <Badge>Not connected</Badge> :
                     failed ? <Badge tone="fail">Can’t connect</Badge> :
                     app.status?.state === "connected" ? <Badge tone="run"><span className="size-1.5 rounded-full bg-run" />Connected</Badge> :
                     <Badge tone="wait">Connecting</Badge>}
                    {app.connected && app.config.workspace && <Badge tone="outline">{app.config.workspace}</Badge>}
                    {app.connected && app.config.bot_name && <Badge tone="outline">@{app.config.bot_name}</Badge>}
                  </div>
                  <p className="mt-0.5 text-[13px] text-muted">{COPY[app.platform].blurb}</p>
                </div>
                {isAdmin && <Button variant={app.connected ? "secondary" : "primary"} onClick={() => setEditing(app)}>{app.connected ? "Change" : "Connect"}</Button>}
              </div>
              {failed && <Notice tone="error" title={`Hyperbeam can’t reach ${title}`}>{app.status?.detail}</Notice>}
              {app.connected && isAdmin && <Channels platform={app.platform} />}
            </Card>
          );
        })}
      </div>
      {editing && <AppDialog app={editing} onClose={() => (setEditing(null), reload())} />}
    </>
  );
}

function AppDialog({ app, onClose }: { app: ChatApp; onClose: () => void }) {
  const { platform } = app;
  const title = CHAT_TITLE[platform];
  const [botToken, setBotToken] = useState("");
  const [appToken, setAppToken] = useState("");
  const [autoLink, setAutoLink] = useState(app.config.auto_link_email ?? true);
  const action = useAction();
  const hint = app.connected ? "Leave blank to keep the current one. Stored encrypted and never shown again." : "Stored encrypted and never shown again.";
  const ready = app.connected || (botToken.trim() && (platform === "discord" || appToken.trim()));
  const save = () =>
    action.run(async () => {
      const tokens: Record<string, string> = {};
      if (botToken.trim()) tokens.bot_token = botToken.trim();
      if (appToken.trim()) tokens.app_token = appToken.trim();
      await api.put(`/chat/${platform}`, { tokens, auto_link_email: platform === "slack" ? autoLink : undefined });
      onClose();
    }, { success: `${title} ${app.connected ? "updated" : "connected"}` });
  const disconnect = async () => {
    const ok = await confirm({
      title: `Disconnect ${title}?`,
      description: `The bot stops answering and its tokens are deleted. Agents started from ${title} keep running and stay on the board. People stay linked.`,
      confirmLabel: "Disconnect",
      destructive: true,
    });
    if (ok) void action.run(async () => (await api.del(`/chat/${platform}`), onClose()), { success: `${title} disconnected` });
  };
  return (
    <Modal
      title={`${app.connected ? "Change" : "Connect"} ${title}`}
      onClose={onClose}
      modalOnly
      footer={
        <>
          {app.connected && <Button variant="danger-ghost" className="mr-auto" disabled={action.busy} onClick={() => void disconnect()}>Disconnect</Button>}
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!ready} loading={action.busy} onClick={() => void save()}>{action.busy ? "Checking" : "Save"}</Button>
        </>
      }
    >
      {platform === "slack" ? (
        <>
          <ol className="grid list-decimal gap-1.5 pl-5 text-[13px] text-muted">
            <li>At api.slack.com/apps, choose <b>Create New App</b> → <b>From a manifest</b>, pick your workspace and paste the manifest below.</li>
            <li>Under <b>Basic Information</b> → <b>App-Level Tokens</b>, generate a token with the <span className="font-mono text-[12.5px]">connections:write</span> scope.</li>
            <li>Under <b>Install App</b>, install it to the workspace and copy the bot token.</li>
            <li>Invite the bot to the channels you want to use it in, then enable them here.</li>
          </ol>
          <Field label="App manifest" hint="Hyperbeam dials out to Slack (Socket Mode), so Slack never needs to reach this server.">
            {(id) => <Textarea id={id} mono readOnly rows={6} value={SLACK_MANIFEST} onFocus={(e) => e.target.select()} />}
          </Field>
          <Field label="Bot token" hint={hint}>
            {(id) => <Input id={id} type="password" mono autoComplete="off" value={botToken} onChange={(e) => setBotToken(e.target.value)} placeholder={app.connected ? "••••••••••••" : "xoxb-…"} />}
          </Field>
          <Field label="App-level token" hint={hint}>
            {(id) => <Input id={id} type="password" mono autoComplete="off" value={appToken} onChange={(e) => setAppToken(e.target.value)} placeholder={app.connected ? "••••••••••••" : "xapp-…"} />}
          </Field>
          <SwitchRow
            checked={autoLink}
            onCheckedChange={setAutoLink}
            label="Link people by email address"
            description="A workspace member whose confirmed Slack address is a team member’s is linked the first time they write to the bot. Guests from other organisations never are. Turned off, everyone links by hand."
          />
        </>
      ) : (
        <>
          <ol className="grid list-decimal gap-1.5 pl-5 text-[13px] text-muted">
            <li>At discord.com/developers/applications, create an application and open <b>Bot</b>.</li>
            <li>Under <b>Privileged Gateway Intents</b>, turn on <b>Message Content Intent</b>. Without it the bot cannot read replies.</li>
            <li>Reset and copy the bot token.</li>
            <li>Under <b>OAuth2</b> → <b>URL Generator</b>, choose the <span className="font-mono text-[12.5px]">bot</span> scope with View Channels, Send Messages, Create Public Threads, Send Messages in Threads, Read Message History and Add Reactions, then open the URL to add the bot to your server.</li>
          </ol>
          <Field label="Bot token" hint={hint}>
            {(id) => <Input id={id} type="password" mono autoComplete="off" value={botToken} onChange={(e) => setBotToken(e.target.value)} placeholder={app.connected ? "••••••••••••" : undefined} />}
          </Field>
          <p className="text-[13px] text-muted">Discord does not tell bots anyone’s email address, so each person links their account by hand the first time they write to the bot.</p>
        </>
      )}
      <ErrorNote error={action.error} />
    </Modal>
  );
}

function Channels({ platform }: { platform: ChatPlatform }) {
  const { data, error, reload } = useLoad<{ channels: ChatChannel[] }>(() => api.get(`/chat/${platform}/channels`), [platform]);
  const [editing, setEditing] = useState<ChatChannel | null>(null);
  const action = useAction();
  const toggle = (channel: ChatChannel, enabled: boolean) =>
    action.run(
      async () => (await api.put(`/chat/${platform}/channels/${channel.id}`, { enabled, name: channel.name, repos: channel.repos, harness: channel.harness, role: channel.role }), reload()),
      { toastError: true },
    );
  return (
    <div className="grid gap-2 border-t border-border pt-3">
      <div className="flex items-baseline justify-between gap-4">
        <h3 className="text-[13px] font-semibold">Channels</h3>
        <span className="text-[12.5px] text-muted">Agents can be started only in the channels you turn on.</span>
      </div>
      <ErrorNote error={error} />
      {!data && !error && <Skeleton className="h-10 rounded-md" />}
      {data && data.channels.length === 0 && (
        <EmptyState compact icon={<Hash />} title="The bot is not in any channel yet">
          Add it to a channel in {CHAT_TITLE[platform]}, then come back to turn that channel on.
        </EmptyState>
      )}
      {data && data.channels.length > 0 && (
        <div className="divide-y divide-border rounded-md border border-border">
          {data.channels.map((channel) => (
            <div key={channel.id} className="flex flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2">
              <span className="min-w-0 flex-1 truncate font-mono text-[13px]">{channel.name}</span>
              {channel.private && <Badge tone="outline"><Lock /> Private</Badge>}
              {channel.external && <Badge tone="wait">Shared with another organisation</Badge>}
              {channel.missing && <Badge tone="wait">Bot is no longer in it</Badge>}
              {channel.enabled && (
                <>
                  {channel.repos.map((repo) => <Badge key={repo} tone="outline" mono>{repo}</Badge>)}
                  {channel.role === "conductor" && <Badge tone="outline">Conductor</Badge>}
                  <Button variant="ghost" size="icon-sm" aria-label={`Defaults for ${channel.name}`} onClick={() => setEditing(channel)}><SlidersHorizontal /></Button>
                </>
              )}
              <Switch aria-label={`Start agents in ${channel.name}`} checked={channel.enabled} disabled={action.busy || (channel.external && !channel.enabled)} onCheckedChange={(on) => void toggle(channel, on)} />
            </div>
          ))}
        </div>
      )}
      {editing && <ChannelDialog platform={platform} channel={editing} onClose={() => (setEditing(null), reload())} />}
    </div>
  );
}

function ChannelDialog({ platform, channel, onClose }: { platform: ChatPlatform; channel: ChatChannel; onClose: () => void }) {
  const repos = useLoad<{ repos: Repo[] }>(() => api.get("/repos"), []);
  const harnesses = useLoad<{ harnesses: Harness[] }>(() => api.get("/harnesses"), []);
  const [picked, setPicked] = useState<string[]>(channel.repos);
  const [harness, setHarness] = useState(channel.harness ?? "");
  const [role, setRole] = useState(channel.role);
  const action = useAction();
  const save = () =>
    action.run(async () => {
      await api.put(`/chat/${platform}/channels/${channel.id}`, { enabled: true, name: channel.name, repos: picked, harness: harness || null, role });
      onClose();
    }, { success: `${channel.name} updated` });
  return (
    <Modal
      title={`Agents started in ${channel.name}`}
      description="What an agent gets when someone asks for one in this channel. It still acts with that person’s own permissions."
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" loading={action.busy} onClick={() => void save()}>Save</Button>
        </>
      }
    >
      <Field label="Repositories" hint="Checked out for every agent started here. Repositories named in the request are added. With none set and none named, the bot asks which one.">
        {() => (
          <div className="flex flex-wrap gap-1.5">
            {repos.data?.repos.map((repo) => (
              <ToggleChip key={repo.id} mono pressed={picked.includes(repo.full_name)} onPressedChange={(on) => setPicked((p) => (on ? [...p, repo.full_name] : p.filter((r) => r !== repo.full_name)))}>
                {repo.full_name}
              </ToggleChip>
            ))}
            {repos.data?.repos.length === 0 && <span className="text-[13px] text-muted">No repositories are registered yet.</span>}
          </div>
        )}
      </Field>
      <Field label="Who does the work">
        {(id) => (
          <Select
            id={id}
            value={role}
            onValueChange={(v) => setRole(v as ChatChannel["role"])}
            options={[
              { value: "worker", label: "One agent", description: "Does the work itself, in its own sandbox." },
              { value: "conductor", label: "Conductor", description: "Scopes the work, then starts and coordinates several agents." },
            ]}
          />
        )}
      </Field>
      <Field label="Harness">
        {(id) => (
          <Select
            id={id}
            value={harness}
            onValueChange={setHarness}
            options={[{ value: "", label: "The team’s default" }, ...(harnesses.data?.harnesses ?? []).map((h) => ({ value: h.name, label: h.title }))]}
          />
        )}
      </Field>
      <ErrorNote error={repos.error ?? harnesses.error ?? action.error} />
    </Modal>
  );
}

// -------------------------------------------------------------- my account

/** The chat accounts that act as the signed-in person. */
export function ChatIdentities() {
  const { data, error, reload } = useLoad<{ identities: ChatIdentity[] }>(() => api.get("/me/chat-identities"), []);
  const action = useAction();
  const unlink = async (identity: ChatIdentity) => {
    const title = CHAT_TITLE[identity.platform];
    const ok = await confirm({
      title: `Unlink ${identity.display_name || title}?`,
      description:
        identity.linked_by === "email"
          ? `This ${title} account can no longer start or direct agents as you. It was linked by its email address and will be linked again the next time it writes to the bot, unless an admin turns that off.`
          : `This ${title} account can no longer start or direct agents as you.`,
      confirmLabel: "Unlink",
      destructive: true,
    });
    if (ok) void action.run(async () => (await api.del(`/me/chat-identities/${identity.platform}/${encodeURIComponent(identity.external_id)}`), reload()), { success: "Account unlinked", toastError: true });
  };
  return (
    <>
      <ErrorNote error={error} />
      {data && data.identities.length === 0 && (
        <p className="text-[13px] text-muted">None yet. Write to the Hyperbeam bot in Slack or Discord and it will send you a link.</p>
      )}
      {data && data.identities.length > 0 && (
        <Card className="divide-y divide-border">
          {data.identities.map((identity) => (
            <div key={`${identity.platform}/${identity.external_id}`} className="flex items-center gap-3 px-3.5 py-2.5">
              <MessagesSquare className="size-4 shrink-0 text-faint" />
              <div className="min-w-0 flex-1">
                <div className="font-medium">{identity.display_name || identity.external_id} <span className="font-normal text-muted">on {CHAT_TITLE[identity.platform]}</span></div>
                <div className="text-[12px] text-muted">Linked <Time iso={identity.linked_at} /> · {identity.linked_by === "email" ? "matched by email address" : "confirmed by you"}</div>
              </div>
              <Button variant="danger-ghost" size="sm" onClick={() => void unlink(identity)}>Unlink</Button>
            </div>
          ))}
        </Card>
      )}
    </>
  );
}

// ----------------------------------------------------------------- linking

/** Where the bot's one-time link lands: confirm that this chat account is you. */
export function LinkPage({ code }: { code: string }) {
  useDocumentTitle("Link your chat account");
  const me = useStore((s) => s.me)!;
  const { data, error } = useLoad<{ platform: ChatPlatform; display_name: string }>(() => api.get(`/chat/link/${encodeURIComponent(code)}`), [code]);
  const [done, setDone] = useState(false);
  const action = useAction();
  const title = data ? CHAT_TITLE[data.platform] : "chat";
  const link = () => action.run(async () => (await api.post(`/chat/link/${encodeURIComponent(code)}`), setDone(true)));
  return (
    <Page width="narrow">
      <div className="grid justify-items-center gap-4 py-20 text-center">
        <div className="grid size-12 place-items-center rounded-full bg-accent text-accent-foreground">{done ? <BrandMark size={26} /> : <Link2 className="size-6" />}</div>
        {error ? (
          <>
            <h1 className="font-display text-2xl font-semibold">This link can’t be used</h1>
            <p className="max-w-[46ch] text-muted">{error}</p>
            <Button variant="secondary" onClick={() => navigate("/")}>Back to the board</Button>
          </>
        ) : !data ? (
          <Skeleton className="h-24 w-full max-w-[420px] rounded-lg" />
        ) : done ? (
          <>
            <h1 className="font-display text-2xl font-semibold">You’re linked</h1>
            <p className="max-w-[46ch] text-muted">Go back to {title} and ask the bot again. You can unlink the account under <A href="/settings/account" className="text-primary hover:underline">My account</A>.</p>
          </>
        ) : (
          <>
            <h1 className="font-display text-2xl font-semibold">Link {title} to your account?</h1>
            <p className="max-w-[50ch] text-muted">
              The {title} account <b className="text-foreground">{data.display_name}</b> will be able to start and direct agents as <b className="text-foreground">{me.name}</b> ({me.email}), with your permissions.
            </p>
            <Notice tone="warning" className="max-w-[460px] text-left">Only continue if you asked the bot for this link yourself. If someone sent it to you, it would give their chat account your permissions.</Notice>
            <ErrorNote error={action.error} />
            <div className="flex gap-2">
              <Button onClick={() => navigate("/")}>Cancel</Button>
              <Button variant="primary" loading={action.busy} onClick={() => void link()}>Link account</Button>
            </div>
          </>
        )}
      </div>
    </Page>
  );
}
