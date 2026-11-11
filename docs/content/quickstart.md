---
title: Quick start
description: Run Noemune on your laptop and watch an agent work, without a model key.
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

## 4. Start an agent with the mock harness

The **Mock harness** runs a script instead of a model, so you can see Noemune work before adding a key.

1. Press <kbd>C</kbd>, or select **New work**.
2. Choose **Mock harness** under **Harness**.
3. Paste this script as the request, and select **Start agent**:

```text
!plan look around; report
!sh ls -la
!say Nothing here yet.
```

You land on the agent’s page and watch it set up its sandbox, make a plan, run a command and reply. Send it a message to continue the conversation.

## 5. Try a real agent

In this local mode the harness runs on your machine, so install its ACP adapter. For Claude Code:

```bash
npm i -g @agentclientprotocol/claude-agent-acp
```

Then give the harness a credential under **Settings → Harnesses**, and pick it in **New work**. See [Harnesses and models](/harnesses) and [Credentials](/credentials).

## Next steps

- [Concepts](/concepts) explains workstreams, sessions, sandboxes and grants.
- [Set up a team](/team-setup) connects GitHub and invites people.
