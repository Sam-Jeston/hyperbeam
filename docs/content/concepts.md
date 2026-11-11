---
title: Concepts
description: Workstreams, agents, sandboxes, harnesses and grants, and how they relate.
---

A handful of ideas carry the rest of Hyperbeam.

## Workstreams

A **workstream** is a piece of work the team is tracking: a feature, a bug, an investigation. It has a title, a brief, the repositories it touches, the agents working on it, a discussion for people, and a stage on the [board](/board): *Scoping*, *Building*, *In review*, *Done* or *Archived*.

Starting work puts it on the board as a new workstream by default. You can also add agents to an existing one.

## Agents and sessions

Each agent you start is a **session**: one conversation with one harness, in one sandbox. Its page shows everything it does as it happens. A session is always in one of these states:

| State | What it means |
|---|---|
| Queued | Waiting for a free sandbox. |
| Starting | Its sandbox is being prepared. |
| Working | The agent is working on its turn. |
| Waiting for input | The agent finished its turn and is waiting for a reply. |
| Asleep | Its sandbox shut down after being idle. A message wakes it in a fresh one. |
| Failed | The session stopped because of an error. |
| Stopped | Someone stopped it. |

A session is either a **worker**, which does the work itself, or a **conductor**, which scopes the work and starts workers. See [Conductors](/conductor).

## Sandboxes

Each live session runs in its own **sandbox**: a pod on Kubernetes, or a local process in development. The sandbox checks out the session’s repositories on its own branch, installs its skills, and runs the harness. It can reach the control plane and nothing else, except through Hyperbeam’s proxies.

Sandboxes are torn down after 15 idle minutes. Work survives a teardown only if it was pushed; when you message the session again, it resumes in a fresh sandbox from the branch it pushed.

## Harnesses

A **harness** is the coding agent that runs inside the sandbox: Claude Code, Codex, Gemini CLI, OpenCode, or any agent that speaks the Agent Client Protocol (ACP) on stdio. A harness profile says which container image to use, how to start the agent, what credential it takes, which models it offers and which hosts it may reach. See [Harnesses and models](/harnesses).

## Grants

A **grant** is one thing an agent may do outside its sandbox, such as `github:open_pr` or `linear:write`. People hold grants, set by an admin. A session’s grants are fixed at launch to what its owner holds, optionally narrowed, and narrowed further if the owner later loses a grant. See [People and permissions](/permissions).

## The control plane

Everything an agent does outside its sandbox goes through the **control plane**: the tool gateway for GitHub, Notion and connected MCP servers, the git proxy for clone and push, and the egress proxy for everything else on the network. Each request is checked against the session’s grants and recorded in the audit log. See [Security model](/security).

## What the team teaches its agents

- **Skills**: instructions an agent reads when the work calls for it, in the open `SKILL.md` format.
- **Team notes**: durable knowledge every agent reads, and adds to as it learns.
- **Workflows**: saved, parameterised ways of starting agents.

See [Skills and team notes](/library) and [Workflows](/workflows).
