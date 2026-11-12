---
title: People and permissions
description: Roles, the grants that decide what each person's agents may do, and per-repository access.
---

An agent can do only what the person who started it is allowed to. You set that per person, under **People & permissions**.

## Roles

| Role | Can |
|---|---|
| Member | Start agents, and build skills, workflows and team notes. |
| Admin | Also manage people, integrations, repositories and harnesses. |
| Owner | Everything, including making and managing other admins. The person who set up the installation. |

An admin’s own agents are still limited to the grants ticked for them. Being an admin does not let an agent do more.

## Grants

| Grant | Lets an agent |
|---|---|
| `github:read` | clone registered repositories; read issues and pull requests |
| `github:write` | push branches (not the default branch); comment; create issues |
| `github:open_pr` | open pull requests |
| `github:merge_pr` | merge pull requests; push to the default branch |
| `web:read` | read public web pages with `web_fetch`. See [Web access](/security#web-access) |
| `<connection>:read` / `<connection>:write` | use a [connection](/integrations#connected-services)’s read / write tools, e.g. `linear:write`, `notion:read` |

Each connection adds its own two columns to **People & permissions**. Only permissions on GitHub, the web (unless an admin has turned web access off) and connections that exist can be granted.

The person who sets up the installation starts with every GitHub grant and `web:read`. Everyone else holds what they are given.

Every stronger grant depends on `read` for its system. Ticking one ticks `read` too, and unticking `read` removes everything else for that system.

## Repository access

By default a person’s agents can reach every registered repository, including ones added later. To limit someone, select **All** in the **Repositories** column and choose **Only the ones I pick**.

## How grants apply to sessions

- A session’s grants are fixed at launch to what its owner holds, optionally narrowed in **New work** or with `--grant`.
- They are evaluated against what the owner holds **now**. If the owner loses a grant, their running agents lose it immediately.
- A worker started by a [conductor](/conductor) can hold at most what the conductor holds.
- Everyone on the team can watch any session. Only its owner, an admin, or someone who holds at least the same grants can direct it.

## Removing someone

**Remove** signs the person out and stops their agents. Their history stays, and you can restore them later.

## The audit log

**Settings → Audit log** lists every attempt by an agent to act outside its sandbox: tool calls, git operations and blocked network requests, with who the agent acted for and whether it was allowed. Turn on **Blocked only** to see what was refused.
