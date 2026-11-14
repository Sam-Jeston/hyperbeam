---
title: Testing and limits
description: How Hyperbeam is tested, and the limits to know about.
---

## Automated tests

The end-to-end tests start a real server, real runner processes driving a scripted ACP agent, and a fake GitHub and GitLab that serve the git smart-HTTP protocol. They cover:

- the permission model;
- sign-up, invites and roles;
- the full session lifecycle through the runner and ACP;
- cloning and pushing through the git proxy, including default-branch protection;
- opening pull requests through the gateway;
- GitLab alongside GitHub: a project under nested groups, its own permissions, and opening and merging merge requests;
- MCP connections against fake upstream servers: tool review, grants, read calls through a read-only variant, changed tools hidden again, and permissions removed on disconnect;
- connecting by OAuth against a fake authorization server: discovery, client registration, PKCE, and renewing a token the server has revoked;
- denial and audit of what is not permitted;
- immediate revocation;
- resuming after teardown;
- conductor delegation;
- questions from agents, answered by people or, for a worker, by its conductor;
- notifications when an agent asks, finishes a turn or fails, in Hyperbeam and by chat direct message;
- reading the web through the gateway’s `web_fetch`, with its URL rules;
- choosing an environment per repository, and running setup commands;
- the master key: generated and wrapped on first start, and rotated between a direct key and wrapped ones;
- Slack and Discord against stand-ins for both: linking by email and by one-time link, refusing people who are not linked, starting agents in enabled channels, replies, questions answered with buttons, and stopping;
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
- A connection has one credential or one OAuth sign-in, shared by every agent, so agents act as that one account. Stdio-only MCP servers can’t be connected yet.
- Workspaces are ephemeral, so work survives a teardown only if it was pushed.
- The Slack and Discord apps are tested against stand-ins, not yet against the live services. One workspace or server set-up per installation.
- The command line has no automated tests of its own; the API it calls is covered.
