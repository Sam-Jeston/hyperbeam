---
title: Slack and Discord
description: Start agents from a channel, reply to them in the thread, and answer their questions without leaving chat.
---

Hyperbeam can run a bot in your Slack workspace or Discord server. Mention it with a request and it starts an agent; the thread under your message becomes the conversation with that agent. Everything you can do there you can also do on the agent’s [page](/sessions), and the work shows on the [board](/board) either way.

This is not the same as the Slack [connection](/integrations#connected-services). A connection lets agents use Slack as a tool. A chat app lets people use agents from Slack.

## Start an agent

In a channel where the bot is enabled, mention it and describe the outcome, the way you would under [New work](/starting-work):

```text
@Hyperbeam Fix the flaky checkout test in acme/web. Don't change the payment mocks.
```

- **Repositories.** The agent checks out the channel’s default repositories, plus any registered repository you name as `owner/name`. If there are none of either, the bot asks which one, and you choose with a button.
- **In a thread.** Mention the bot inside an existing thread and the agent is shown the earlier messages as background. This is how to hand it an alert or a discussion: reply to it with `@Hyperbeam fix this`.
- **Direct messages.** On Slack you can also message the bot directly. Each new message starts an agent; reply in its thread to continue. On Discord, start agents from a server channel: the bot opens a thread on your message.

The agent is yours. It runs with your permissions on the team’s default harness (or the channel’s), and its work goes on the board.

## In the thread

The bot keeps one status message at the top of the thread up to date: whether the agent is working, waiting for you, asleep or stopped, its repositories and branch, and links to what it produced. **Watch** opens the agent’s page, where the full transcript is. **Interrupt** and **Stop** do what they do [there](/sessions#interrupt-or-stop).

After that the thread gets what matters, not everything:

- what the agent says at the end of each turn,
- its **questions**, with a button for each answer it suggested, and
- errors.

**Reply in the thread** to direct the agent. Every reply from someone who may direct it is its next prompt; you do not need to mention the bot again. A reply also answers an open question, and wakes an agent that is asleep. The bot marks a message it has passed on with 👀.

> [!NOTE]
> A reply from someone who may not direct the agent is left alone. The thread is still an ordinary thread for them, and nothing they write reaches the agent.

## Who you are

The bot acts for people, so it has to know which Hyperbeam account a chat account belongs to. **Someone who is not linked can do nothing**: the bot starts no agent for them and passes on nothing they write.

- **By email (Slack).** A member of the workspace whose confirmed Slack address is a team member’s address is linked the first time they write to the bot. Guests from other organisations never are. An admin can turn this off in the Slack app’s settings.
- **By link.** Otherwise the bot sends you a private link that works once, for 15 minutes. Open it while signed in to Hyperbeam and confirm. Discord does not tell bots anyone’s address, so on Discord everyone links this way.

> [!WARNING]
> Only open a link you asked the bot for yourself. Whoever opens it gives that chat account their own permissions.

See and remove your linked accounts under **Settings → My account**.

A linked account is also where the bot can send your [notifications](/notifications), if you turn that on.

Being linked makes you yourself, no more. Directing an agent from chat takes the same standing as [directing it from its page](/sessions#who-can-direct-an-agent): you are its owner, an admin, or hold at least the grants it does.

## Set up Slack

An admin does this once, under **Settings → Chat apps → Slack → Connect**.

1. At *api.slack.com/apps*, choose **Create New App → From a manifest**, pick your workspace, and paste the manifest shown in the dialog.
2. Under **Basic Information → App-Level Tokens**, generate a token with the `connections:write` scope. It starts with `xapp-`.
3. Under **Install App**, install the app to the workspace and copy the bot token. It starts with `xoxb-`.
4. Paste both tokens into the dialog and save. Hyperbeam checks them before storing them.
5. Invite the bot to the channels you want it in, then [enable them](#channels).

Hyperbeam connects to Slack with Socket Mode: it dials out and Slack never needs to reach your server. The manifest asks for these bot scopes: `channels:history`, `groups:history` and `im:history` to see messages in channels the bot has been added to and in its direct messages, `chat:write` and `reactions:write` to answer, `channels:read` and `groups:read` to list the channels it is in, and `users:read` with `users:read.email` for names and for linking by email.

## Set up Discord

Under **Settings → Chat apps → Discord → Connect**.

1. At *discord.com/developers/applications*, create an application and open **Bot**.
2. Under **Privileged Gateway Intents**, turn on **Message Content Intent**. Without it the bot cannot read replies, and Hyperbeam will report that it can’t connect.
3. Reset and copy the bot token, paste it into the dialog and save.
4. Under **OAuth2 → URL Generator**, choose the `bot` scope with *View Channels*, *Send Messages*, *Create Public Threads*, *Send Messages in Threads*, *Read Message History* and *Add Reactions*. Open the URL it gives you to add the bot to your server.
5. [Enable](#channels) the channels you want it in.

Hyperbeam connects to the Discord Gateway by dialling out, as it does with Slack.

## Channels

What an agent says in a thread is read by everyone in the channel, whoever they are in Hyperbeam. So agents can be started only in channels an admin has turned on, listed under the connected app in **Settings → Chat apps**. The list shows the channels the bot is in.

For each enabled channel you can set what agents started there get:

- **Repositories** checked out for every agent.
- **Who does the work**: one agent, or a [conductor](/conductor).
- **Harness**, if not the team’s default.

A Slack channel shared with another organisation cannot be enabled, and the bot refuses to start agents in one.

> [!TIP]
> Give each team’s channel its repositories. Then “@Hyperbeam the nightly build is red” is a complete request.

## What the bot can and cannot do

- The bot’s tokens are stored encrypted in the control plane, like every other credential, and are never given to an agent.
- The bot only speaks in threads it was asked into, in private notes to the person it is answering, and in direct messages to people who asked it for their notifications.
- Messages posted by apps, including an agent posting through the Slack connection, are never taken as prompts. Only what a person typed is.
- Earlier messages of a thread are shown to the agent as background, marked as such. URLs in them are not ones “a person gave the agent” for the purposes of [web access](/security).
- Disconnecting a chat app deletes its tokens and its channel settings, and turns off the notifications people had it send them. Agents it started keep running and stay on the board. People stay linked.
