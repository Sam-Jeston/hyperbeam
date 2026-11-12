---
title: Start work
description: Describe an outcome and start an agent on it, from the board, a workstream or the command line.
---

Everything starts from **New work**. Select it in the sidebar, or press <kbd>C</kbd> anywhere in Hyperbeam.

## Describe the outcome

Write the request the way you would brief a colleague: what you want, anything it must not break, and how you will know it is done.

```text
Fix the flaky checkout test. Don't change the payment mocks.
Done when the checkout suite passes ten runs in a row.
```

## Choose who does the work

- **One agent** does the work itself, in its own sandbox. Right for most requests.
- **Conductor** scopes the work, then starts and coordinates several agents. Best for work that spans repositories. See [Conductors](/conductor).

## Choose repositories, harness and model

- **Repositories**: the ones the agent checks out. You see only the repositories you have access to.
- **Harness**: the coding agent to run. The team default is preselected. A harness that has no credential yet can’t be started; [add one](/credentials) first.
- **Model**: leave it on the default, or choose one the harness offers. See [Models](/harnesses#models).

## Narrow what it may do

Open **Skills and permissions** to:

- pick **skills** to install in the sandbox, and
- untick **permissions** the agent does not need. An agent can never do more than you can, so the list shows only what you hold.

Checking out repositories needs *Read repositories* on the host they are on (`github:read` or `gitlab:read`).

## Put it on the board

**Put this on the board** is on by default. It creates a [workstream](/board) so the team can follow and discuss the work. Give it a title, or let Hyperbeam use the first line of your request.

Select **Start agent** (or press <kbd>Ctrl</kbd>+<kbd>Enter</kbd>). You land on the agent’s page and can [watch it work](/sessions).

## Add an agent to a workstream

On a workstream’s page, select **Add an agent**. The new agent is given the workstream’s brief and repositories, so you only describe the next piece of work.

## Run a workflow

If your team has [workflows](/workflows), **New work** has a **Run a workflow** tab. Choose one and fill in its inputs; the workflow sets the repositories, skills, permissions and prompt.

## From the command line

```bash
hyperbeam run "Fix the flaky checkout test" --repo acme/web --follow
hyperbeam run "Add dark mode" --conductor --repo acme/api --repo acme/web --workstream "Dark mode"
```

See [Command line](/cli).
