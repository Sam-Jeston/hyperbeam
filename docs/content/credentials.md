---
title: Credentials
description: Team and personal keys for harnesses, how they are stored, and what to know about subscription tokens.
---

A harness needs a credential, such as a model provider API key, to run. You can set one for the whole team, and each person can set their own.

## Team and personal credentials

Under **Settings → Harnesses**, each harness shows two lines:

- **Team credential**: used by everyone’s agents that don’t have their own. Only admins can set it.
- **Mine**: used by your agents only, instead of the team’s.

When a session starts, Hyperbeam uses its owner’s personal credential if there is one, and the team credential otherwise.

## Subscription tokens

Some harness profiles also accept a personal subscription token (for Claude Code, from `claude setup-token`) as well as an API key. Hyperbeam treats these as personal: only their owner can set one, for their own sessions, never as a team credential.

> [!WARNING]
> Whether a subscription may be used this way is decided by your provider’s terms, which changed more than once during 2026. For a team or anything automated, use API keys.

## How credentials are stored

Credentials are encrypted at rest with AES-256-GCM, using the installation’s master key. Each one is bound to its owner and harness, so a stored credential can’t be moved to another person or harness. They are never shown again after you save them.

Integration credentials for GitHub, Linear and Notion stay in the control plane and never enter a sandbox. See [Security model](/security).

## What the agent can see

A harness credential is delivered to the sandbox of a session that needs it, as the environment variable the harness profile names. The agent process can therefore read it. The [egress allowlist](/security#network-egress) limits where it can send it, but treat harness keys as visible to the agents that use them, and prefer keys scoped to their purpose.

## The master key

The master key is 32 random bytes, base64-encoded, set with `HYPERBEAM_MASTER_KEY`. Generate one with `hyperbeam-server gen-key` or `openssl rand -base64 32`. The Helm chart keeps it in a Kubernetes Secret.

> [!IMPORTANT]
> Back the master key up, and don’t change it. Hyperbeam can’t yet rotate it: credentials stored under one key can’t be read with another, and would have to be entered again.
