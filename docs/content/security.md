---
title: Security model
description: What a sandbox holds, what it can reach, and how every action outside it is checked and recorded.
---

Hyperbeam is built so that an agent can do only what the person who started it may, and so that agents never hold your integration credentials.

## What a sandbox holds

A session’s sandbox holds exactly one Hyperbeam credential: a **run token** that identifies the session. With it the runner dials out to the control plane; nothing connects in. The token is revoked the moment the session ends.

The sandbox also receives the [harness credential](/credentials#what-the-agent-can-see) its agent needs to call its model provider.

## Every way out goes through Hyperbeam

| Path | What it is for | What is checked |
|---|---|---|
| `/git/…` | `git clone`, `fetch`, `push` | The repository is one of the session’s. `github:read` to fetch, `github:write` to push, `github:merge_pr` to push to the default branch. |
| `/mcp` | Tools for GitHub, Notion, connected MCP servers and the platform | The grant each tool needs, and the repository. For a connection, that an admin has cleared the tool. |
| Egress proxy | Everything else on the network | The host is on the allowlist. |

GitHub, Notion and connection credentials stay in the control plane. The git proxy and tool gateway add them to requests they have checked; the agent never sees them. Connected MCP servers are called by the control plane, not the sandbox, so they need no place on the egress allowlist.

A custom connection’s tools are hidden until an admin approves each one, and hidden again if the server changes one afterwards. See [Custom connections](/integrations#custom-connections).

## Grants are checked as they are now

Grants are evaluated against what the session’s owner holds **now**, not only at launch. Removing a permission from a person takes effect on their running agents immediately. See [People and permissions](/permissions).

## Network egress

On Kubernetes, a NetworkPolicy lets sandboxes reach only the control plane and cluster DNS. With the egress proxy enabled, everything else goes through an allowlist: package registries by default, plus hosts admins add under **Settings → General**, plus each harness profile’s own hosts. Requests to other hosts are refused and recorded.

The NetworkPolicy relies on a CNI that enforces it. See [Network isolation](/install-kubernetes#network-isolation).

## The sandbox itself

Each Kubernetes sandbox is a pod that runs as a non-root user, with no service account token and no added capabilities, under the `restricted` Pod Security Standard, with CPU, memory and disk limits. The control plane’s own permissions in the sandbox namespace are limited to creating and deleting pods and their token secrets.

## The audit log

Every attempt by an agent to act outside its sandbox is recorded: tool calls, git operations and blocked network requests, with the session, the person it acted for, and whether it was allowed. Admins see it under **Settings → Audit log**.

## Stored credentials

Credentials are encrypted at rest with AES-256-GCM under the installation’s master key, each bound to the record it belongs to. The master key can be supplied directly, read from AWS Secrets Manager, or kept encrypted by an AWS KMS key so that it exists unencrypted only in the server’s memory, and it can be [rotated](/credentials#rotating-the-master-key). See [Credentials](/credentials#the-master-key). People’s passwords are hashed with Argon2, and API keys and sign-in tokens are stored only as hashes.

## Known limits

- One team per installation, and the control plane runs as a single replica.
- Sign-in is email and password only; there is no OIDC yet.
- Notion and connections use a key rather than an OAuth flow, so agents act as that key’s user. Servers that sign in only with OAuth can’t be connected yet.
