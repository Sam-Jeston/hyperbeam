---
title: Harnesses and models
description: Which coding agents Hyperbeam runs, how to add your own, and how models are chosen.
---

A **harness** is the coding agent that runs inside each sandbox. Hyperbeam drives any agent that speaks the [Agent Client Protocol](https://agentclientprotocol.com) (ACP) on stdio. Harnesses are managed under **Settings → Harnesses**.

## Built-in profiles

Hyperbeam ships profiles for **Claude Code**, **Codex**, **Gemini CLI** and **OpenCode**. Each needs a [credential](/credentials) before it can start.

On Kubernetes, set each profile’s image to one you have built and pushed. See [Install on Kubernetes](/install-kubernetes#build-and-push-the-images).

## Add a harness

Select **Add a harness** and fill in:

| Field | Meaning |
|---|---|
| Name | Lowercase letters, digits and dashes. Used in workflows and `--harness`. |
| Title, description | How it appears in **New work**. |
| Container image | Needs `git` and the agent itself. Hyperbeam adds its runner at start-up. To work with [environments](/environments), it must keep the agent under `/opt/harness`. |
| Command | JSON array that starts the agent speaking ACP on stdio, such as `["my-agent", "--acp"]`. |
| Default model | Used when someone starts work without choosing a model. |
| Model environment variable | Optional. Also set to the chosen model, for agents that read it from the environment. |
| Models people may choose | Optional. One model id per line, optionally as `id=Label`. Listing any restricts the choice to these. |
| Credentials it accepts | One per line as `ENV_NAME=Label`. Add `[oauth]` at the end for a personal subscription token. |
| Hosts it may reach | One per line. Added to the egress allowlist for this harness’s sandboxes. |

`deploy/docker/Dockerfile.harness` is a starting point for an image. It installs the agent with `harness-install`, which puts it under `/opt/harness` with a Node.js of its own. An agent that is not an npm package can be placed there by hand: its launcher in `/opt/harness/bin`, named as the first word of the command, and everything it needs beside it.

To give agents your language toolchains, use an [environment](/environments) rather than adding them to the harness image. One environment then serves every harness.

## Models

Each session runs on a model chosen when it starts: in **New work**, with `hyperbeam run --model`, or with `model:` in a workflow. A harness profile can set the default used when nobody chooses, and can limit the choice to a list.

The list offered comes from the harness itself. Hyperbeam reads the standard ACP `model` session option and remembers what each harness reported. It asks as soon as it can: when a harness is given a credential, or its image or command changes, Hyperbeam starts the harness for a few seconds in a sandbox of its own, records the list and removes the sandbox. **Refresh** under **Settings → Harnesses** asks again.

That sandbox is not a session: it has no repositories, tools or permissions.

A model the harness does not recognise does not fail the session. It runs on the harness default and says so in its log.

A conductor’s workers inherit its model unless it chooses another for a piece of work.
