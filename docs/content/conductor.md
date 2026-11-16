---
title: Conductors
description: One agent that scopes a request, starts workers across repositories, and is told when each reports back.
---

A **conductor** is an agent that coordinates other agents. Give it a request that spans repositories or has several separable parts. It scopes the work, starts **worker** agents for each piece, and is told as each one reports back, so it can check the results and decide what happens next.

## Start a conductor

In **New work**, choose **Conductor** instead of **One agent**. From the command line:

```bash
hyperbeam run "Add dark mode" --conductor --repo acme/api --repo acme/web --workstream "Dark mode"
```

## What its workers may do

A worker started by a conductor can hold at most what the conductor holds. If the conductor can’t merge pull requests, neither can its workers.

Workers use the conductor’s model unless the conductor chooses another for a piece of work.

## Reviewers and shared branches

A conductor can start a worker that may do less than it can, such as a reviewer that reads the code but can't push. It can put a worker on another worker's branch, and ask it to end with a verdict instead of only a report. You can ask for this in your prompt:

```text
Have one agent build it, then a second, read-only agent review that branch and give a verdict.
```

To make this a fixed part of how a kind of work is done, declare the agents in a [workflow](/workflows#agents), and add a [loop](/workflows#loops) to have Hyperbeam count the rounds and stop the work being called done before the review approves.

## Questions from workers

When a worker asks a question, its conductor is prompted with it and answers by messaging the worker. If the decision is not the conductor's to make, it asks you instead and passes your answer on. You can also answer a worker directly from its page. See [Questions from the agent](/sessions#questions-from-the-agent).

## Following the work

Workers appear on the conductor’s page under **Agents it started**, and nested under the conductor on the workstream’s page. Each worker has its own page, transcript and sandbox, so you can watch or direct any of them.

> [!TIP]
> Conductors work best with a clear definition of done. Ask for a pull request per repository and a short report of the links, as the `ship-feature` [workflow](/workflows) does.
