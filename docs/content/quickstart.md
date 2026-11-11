---
title: Quick start
description: Run Noemune on your laptop and connect your first coding agent.
---

This runs agents as ordinary local processes with **no isolation**. It is for trying Noemune out. To run it for a team, [install it on Kubernetes](/install-kubernetes).

## Before you start

You need [Docker](https://docs.docker.com/get-docker/) (for Postgres), [Rust](https://rustup.rs) and [Bun](https://bun.sh).

## 1. Start Postgres

```bash
docker run -d --name noemune-pg -e POSTGRES_USER=noemune -e POSTGRES_PASSWORD=noemune \
  -e POSTGRES_DB=noemune -p 127.0.0.1:5440:5432 postgres:16
```

## 2. Build Noemune

The web UI is embedded in the server binary, so build it first.

```bash
(cd web && bun install && bun run build)
cargo build --release
```

## 3. Start the server

```bash
export NOEMUNE_DATABASE_URL=postgres://noemune:noemune@127.0.0.1:5440/noemune
export NOEMUNE_MASTER_KEY=$(./target/release/noemune-server gen-key)
./target/release/noemune-server serve
```

> [!WARNING]
> Keep the master key. It encrypts every stored credential, and if it changes, the credentials stored with the old one can no longer be read. See [Credentials](/credentials#the-master-key).

Open <http://localhost:8080>. The first person in creates the team and owns it.

## 4. Connect an agent

Noemune drives coding agents over the [Agent Client Protocol](https://agentclientprotocol.com) (ACP). In this local mode the agent runs on your machine, so install its ACP adapter. For Claude Code:

```bash
npm i -g @agentclientprotocol/claude-agent-acp
```

Then give it a credential:

1. Open **Settings → Harnesses** and select **Claude Code**.
2. Add an Anthropic API key, for the team or just for yourself. Or add your own Claude subscription token from `claude setup-token`; it is used only for your sessions.

Codex, Gemini CLI and OpenCode work the same way. See [Harnesses and models](/harnesses) and [Credentials](/credentials).

## 5. Start your first agent

1. Press <kbd>C</kbd>, or select **New work**.
2. Describe the task, and pick **Claude Code** under **Harness** if it is not already the default.
3. Select **Start agent**.

You land on the agent’s page and watch it set up its sandbox, plan, run commands and reply. Send it a message to continue the conversation. To let agents clone your repositories and open pull requests, connect GitHub under **Settings → Integrations**. See [Set up a team](/team-setup).

## Next steps

- [Concepts](/concepts) explains workstreams, sessions, sandboxes and grants.
- [Set up a team](/team-setup) connects GitHub and invites people.
