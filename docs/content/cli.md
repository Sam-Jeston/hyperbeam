---
title: Command line
description: Start, follow, message and stop agents from your terminal or CI.
---

The `noemune` command talks to a Noemune server with an API key that acts as you, so agents started from the command line have exactly the permissions they would have from the UI.

## Sign in

Create an API key under **Settings → My account → API keys**, then:

```bash
noemune login --server https://noemune.example.com
```

You are asked for the key. `NOEMUNE_SERVER` and `NOEMUNE_API_KEY` in the environment take the place of `noemune login`, which is handy in CI. `noemune whoami` shows who you are signed in as.

## Start an agent

```bash
noemune run "Fix the flaky checkout test" --repo acme/web --model sonnet --follow
```

| Option | Meaning |
|---|---|
| `--repo <owner/name>` | Repository to check out. Repeatable. |
| `--skill <name>` | Skill to install. Repeatable. |
| `--harness <name>` | Harness profile. Defaults to the team’s. |
| `-m`, `--model <model>` | Model to run on. Defaults to the harness profile’s. |
| `--conductor` | Start a conductor that delegates to worker agents. |
| `--workstream <title>` | Put it on the board as a new workstream with this title. |
| `--grant <grant>` | Limit the agent to these permissions, such as `github:read`. Repeatable. Defaults to everything you hold. |
| `-f`, `--follow` | Stream the agent’s work until its turn ends. |

## Run a workflow

```bash
noemune workflow ship-feature -i feature="CSV export" --follow
```

| Option | Meaning |
|---|---|
| `-i`, `--input <name=value>` | An input. Repeatable. |
| `-m`, `--model <model>` | Model to run on, overriding the workflow’s. |
| `--workstream <title>` | Put it on the board as a new workstream with this title. |
| `-f`, `--follow` | Stream the agent’s work until its turn ends. |

## Manage sessions

```bash
noemune ls                     # sessions that are live now
noemune ls --all --mine        # include finished ones; only yours
noemune logs -f 3ff6c49b       # print a session's log, and keep following it
noemune send 3ff6c49b "Also update the changelog" --follow
noemune interrupt 3ff6c49b     # end the current turn, keep the session
noemune stop 3ff6c49b          # stop it and tear down its sandbox
```

A session id can be shortened to any part that matches only one session: its first characters, or its last. Ids start with a timestamp, so the last eight characters are usually the easiest to tell apart; `noemune ls` shows them.
