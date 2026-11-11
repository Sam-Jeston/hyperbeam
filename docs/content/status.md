---
title: What is verified
description: What has been tested, how, and what has not been tested yet.
---

Noemune is early. This page says plainly what has been verified and what has not, so you know what to expect.

## Verified by automated tests

The end-to-end tests start a real server, real runner processes and a fake GitHub that serves the git smart-HTTP protocol. They cover:

- the permission model;
- sign-up, invites and roles;
- the full session lifecycle through the runner and ACP, with the mock harness;
- cloning and pushing through the git proxy, including default-branch protection;
- opening pull requests through the gateway;
- denial and audit of what is not permitted;
- immediate revocation;
- resuming after teardown;
- conductor delegation;
- model selection;
- the egress allowlist.

The UI was exercised in a browser against the mock harness.

## Verified by hand on Kubernetes

On a local `kind` cluster (Kubernetes 1.34) with the Helm chart:

- the control plane and bundled Postgres come up;
- a session runs in its own pod as a non-root user with no service account token;
- allowlisted hosts are reachable through the egress proxy, and others are refused, both through the proxy and directly;
- a live session survives a control plane restart;
- stopping a session removes its pod and token secret.

## Not yet verified

These are likely to need fixes on first contact:

- **A real harness.** The ACP client follows the published v1 schema but has only driven the mock.
- **Linear, Notion and GitHub App mode.** Written against the documented APIs; never called against the real services.
- **The harness images.** `Dockerfile.harness` has not been built for each agent.

## Known limits

- One team per installation.
- The control plane runs as a single replica.
- Sign-in is email and password only (no OIDC yet).
- Linear and Notion connect with a key rather than an OAuth flow, so agents act as that key’s user.
- Workspaces are ephemeral, so work survives a teardown only if it was pushed.
