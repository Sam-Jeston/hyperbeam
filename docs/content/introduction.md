---
title: Introduction
description: What Noemune is, who it is for, and how it is different from running an agent in your own terminal.
---

Noemune is a self-hosted platform where a team’s coding agents work in the open.

Today most agent work happens in someone’s terminal and stays invisible until a pull request appears. Noemune moves that work one level up: anyone on the team can describe an outcome, an agent picks it up in its own sandbox, and everyone can watch the scoping and the building as it happens.

## What you get

- **A shared board.** Workstreams move from scoping to building to review. Each shows the agents on it, what they are doing right now, and the pull requests they produced. See [The board and workstreams](/board).
- **Any harness.** Claude Code, Codex, Gemini CLI, OpenCode, or anything else that speaks the [Agent Client Protocol](https://agentclientprotocol.com). Bring a team API key or your own credential. See [Harnesses and models](/harnesses).
- **Permissions that belong to people.** An agent can do only what the person who started it may: GitHub, Linear and Notion, per repository. See [People and permissions](/permissions).
- **Agents never hold your credentials.** GitHub, Linear and Notion are reached through Noemune’s tool gateway and git proxy, which check every request and record it. See [Security model](/security).
- **A conductor.** One agent that scopes a request, starts worker agents across repositories, and is told when each reports back. See [Conductors](/conductor).
- **Skills, workflows and team notes.** What your team teaches its agents, kept in one place and shared by every session. See [Skills and team notes](/library) and [Workflows](/workflows).

## Who it is for

Teams that already use coding agents and want that work to be visible, shared and governed: engineering teams that want to see what agents are doing across repositories, and people who never open a terminal but can describe what they need built or investigated.

## How it fits together

```text
 Browser / CLI
      │  REST + server-sent events
┌─────▼────────────────── noemune-server ────────────────────────┐
│ API · teams and permissions · session manager · event log      │
│ tool gateway (MCP) · git proxy · egress allowlist proxy        │
└─────┬──────────────────────────────────┬───────────────────────┘
      │ Postgres (all state)             │ one pod per live session
┌─────▼──────────────── agent sandbox ───────────────────────────┐
│ noemune-runner ⇄ ACP over stdio ⇄ harness (Claude Code, …)     │
│ may reach: the control plane. Nothing else, except through it. │
└────────────────────────────────────────────────────────────────┘
```

The **control plane** (`noemune-server`) holds all state in Postgres, serves the web UI and API, and is the only way out of a sandbox. Each live agent session gets its own **sandbox**: a process on your laptop for development, or a locked-down pod on Kubernetes. Inside it, the **runner** drives the harness over ACP and streams everything it does back to the control plane.

> [!NOTE]
> Noemune is early. The control plane, runner, UI and CLI work and are covered by end-to-end tests using a scripted mock harness and a fake GitHub. [What is verified](/status) lists exactly what has and has not been tested.

## Next steps

- [Quick start](/quickstart): run it on your laptop in a few minutes, no model key needed.
- [Concepts](/concepts): the handful of ideas the rest of the docs build on.
- [Install on Kubernetes](/install-kubernetes): set it up for your team.
