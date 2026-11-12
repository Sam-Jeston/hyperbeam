---
title: Watch and direct agents
description: Follow an agent as it works, reply to it, interrupt or stop it, and wake it up later.
---

Every agent has its own page. Open one from the board, a workstream, or **Agents** in the sidebar.

## What you see

The transcript shows everything the agent does, as it happens:

- **Messages** from people, and the agent’s replies.
- **Thinking**, collapsed by default. Select it to read the agent’s reasoning.
- **Tool calls**: files read and edited, commands run, searches. Each shows whether it is running, done or failed. Expand one to see its input and output; edits show the diff and how many lines were added and removed.
- **Permission checks**: every request the agent made outside its sandbox, marked **Allowed**, **Failed** or **Blocked**. Web pages it read appear here with their URL.

The side panel shows the agent’s **plan** and progress through it, who started it, the harness and model, its repositories and branch, links to what it **produced** (such as pull requests), the agents it started, and what it **may do**.

## Reply to the agent

Type in the box at the bottom and press <kbd>Enter</kbd> to send (<kbd>Shift</kbd>+<kbd>Enter</kbd> for a new line).

- If the agent is **waiting for input**, your message starts its next turn.
- If it is **working**, your message is queued and delivered when the turn ends.
- If it is **asleep**, your message wakes it in a fresh sandbox.

## Questions from the agent

An agent can ask a question when a decision is yours to make. The question appears in the transcript, the session shows **Has a question**, and its workstream's card on the board shows the same.

- If the agent offered options, select one to answer. You can always type an answer in your own words instead.
- Any message you send while a question is open counts as the reply.
- The agent does not block while it waits. It finishes what it can and ends its turn, and your answer starts its next one. If its sandbox has shut down in the meantime, the answer wakes it in a fresh one.

A worker started by a [conductor](/conductor) asks its conductor first. The session shows **Asked its conductor**, and you can still answer it yourself.

Answering directs the agent, so it needs the same standing as sending a message. See [Who can direct an agent](#who-can-direct-an-agent).

## Interrupt or stop

- **Interrupt** ends the current turn. The agent stays alive and waits for your next message.
- **Stop** shuts the sandbox down. Unpushed work in it is lost. You can still read the transcript, and a message later starts the agent again in a fresh sandbox.

## Asleep and waking up

A sandbox that has been idle for 15 minutes is torn down, and the session shows as **Asleep**. Send it a message to resume: Hyperbeam starts a fresh sandbox from the branch the agent pushed, and the conversation carries on.

> [!TIP]
> Ask agents to push their work as they go. Workspaces are ephemeral, so only pushed work survives a teardown.

## Who can direct an agent

Everyone on the team can watch any session. Only its owner, an admin, or someone who holds at least the same grants can send it messages, interrupt it or stop it, because prompting an agent means acting with its owner’s permissions. Everyone else sees a note saying they are watching, and can comment on the workstream instead.
