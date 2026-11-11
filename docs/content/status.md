---
title: Testing and limits
description: How Hyperbeam is tested, and the limits to know about.
---

## Automated tests

The end-to-end tests start a real server, real runner processes driving a scripted ACP agent, and a fake GitHub that serves the git smart-HTTP protocol. They cover:

- the permission model;
- sign-up, invites and roles;
- the full session lifecycle through the runner and ACP;
- cloning and pushing through the git proxy, including default-branch protection;
- opening pull requests through the gateway;
- denial and audit of what is not permitted;
- immediate revocation;
- resuming after teardown;
- conductor delegation;
- model selection;
- the egress allowlist.

## Kubernetes

On a local `kind` cluster (Kubernetes 1.34) with the Helm chart:

- the control plane and bundled Postgres come up;
- a session runs in its own pod as a non-root user with no service account token;
- allowlisted hosts are reachable through the egress proxy, and others are refused, both through the proxy and directly;
- a live session survives a control plane restart;
- stopping a session removes its pod and token secret.

## Known limits

- One team per installation.
- The control plane runs as a single replica.
- Sign-in is email and password only (no OIDC yet).
- Linear and Notion connect with a key rather than an OAuth flow, so agents act as that key’s user.
- Workspaces are ephemeral, so work survives a teardown only if it was pushed.
