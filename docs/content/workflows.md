---
title: Workflows
description: Saved, parameterised ways of starting agents, written in YAML.
---

A workflow captures a request your team makes often, so anyone can run it by filling in the blanks. It says which repositories, skills and permissions to use, and what to ask for.

Create and edit workflows under **Library → Workflows**. Run them from **New work → Run a workflow**, or from the command line.

## Example

```yaml title="ship-feature.yaml"
name: ship-feature
description: Build a feature across the API and the web app
role: conductor                # or: worker
repos: [acme/api, acme/web]
skills: [house-style]
grants: [github:write, github:open_pr, linear:read]   # the most its agents may do
inputs:
  - name: feature
    description: What to build, and how you will know it works
prompt: |
  Build this feature: {{feature}}
  Scope it first, then delegate each repository's changes to a worker.
  Open a pull request per repository and report the links.
```

## Fields

| Field | Required | Meaning |
|---|---|---|
| `name` | yes | How people and the CLI refer to it. |
| `description` | no | One line shown when choosing a workflow. |
| `role` | no | `worker` (the default) or `conductor`. |
| `harness` | no | Harness profile name. Defaults to the team’s. |
| `model` | no | Model for its agents. Defaults to the harness profile’s. Whoever runs it can override this. |
| `environment` | no | [Environment](/environments) its agents run in. Defaults to the repositories’. Whoever runs it can override this. |
| `repos` | no | `owner/name` repositories to check out. |
| `skills` | no | Skills to install. |
| `grants` | no | The most its agents may do. Still limited to what the person running it holds. |
| `inputs` | no | Values asked for when it runs. |
| `agents` | no | Named workers its conductor can start. See [Agents](#agents). |
| `loop` | no | An implement-and-review loop over two of those agents. See [Loops](#loops). |
| `prompt` | yes | The request. `{{name}}` is replaced by the input of that name. |

Each input has a `name`, an optional `description`, `required` (true unless set to false) and an optional `default`.

## Agents

A conductor workflow can declare the workers its conductor will start, so each one's brief, skills and permissions are written once by you, not improvised by the conductor each time. The conductor starts one by name and adds only what is particular to that piece of work.

```yaml
agents:
  implementer:
    description: Builds the change
    skills: [house-style]
    prompt: |
      Build this: {{goal}}
      Push your branch before you end each turn.
  reviewer:
    description: Reviews the implementer's branch
    grants: [github:read]
    branch_of: implementer
    verdict: true
    prompt: |
      Review the work on this branch against: {{goal}}
```

| Field | Meaning |
|---|---|
| `prompt` | Its standing brief. Required. `{{name}}` takes the workflow's inputs. |
| `description` | One line telling the conductor what the agent is for. |
| `grants` | The most it may do. It can only narrow what the conductor holds. Leave it out to match the conductor. |
| `skills` | Skills to install. |
| `harness`, `model` | Default to the conductor's. |
| `repos` | Repositories to check out, from the workflow's. Defaults to all of them. |
| `branch_of` | Another agent's name. It checks out the branch of the latest agent started under that name, not a branch of its own. |
| `verdict` | `true` if it must end by reporting a verdict: approve, or request changes with findings. |
| `fresh` | `true` to start a new agent each time, never resuming the last, so it isn't anchored on what it concluded before. |

The gateway enforces `grants`, not the prompt. A reviewer given only `github:read` can't push, however it is asked.

## Loops

A `loop` names two agents, one to implement and one to review, and has the conductor run them in turn until the reviewer approves.

```yaml
loop:
  implement: implementer
  review: reviewer
  max_rounds: 4          # 5 if left out
```

The reviewer always reports a verdict, and reviews the implementer's branch unless its `branch_of` says otherwise, so you don't need to set either.

The conductor still drives: it starts the implementer, decides when the work is ready to review, and passes findings back. Hyperbeam holds the parts that shouldn't depend on its judgement:

- **Rounds are counted.** Each verdict from the reviewer ends a round. After `max_rounds` without an approval, Hyperbeam refuses to start or message either agent, and the conductor is told to stop and ask you. The conductor's page shows **Out of rounds**, where you can allow more.
- **Done means approved.** Until the reviewer's latest verdict approves, the workstream can't be moved to review or done.
- **A review ends in a verdict.** If the reviewer ends a turn without one, the conductor is told to ask for it.

The conductor's page shows the round the loop is in and what each review concluded.

```yaml title="build-and-review.yaml"
name: build-and-review
description: Build a change, have it reviewed, and repeat until the review approves
role: conductor
repos:
  - acme/api
grants:
  - github:write
  - github:open_pr
inputs:
  - name: goal
    description: What to build, and how you will know it works
agents:
  implementer:
    description: Builds the change and fixes what the reviewer finds
    prompt: |
      Build this: {{goal}}

      Work in small commits and run the tests. Push your branch before you end each turn,
      because the reviewer reads what is pushed. Do not open a pull request.
      When you are sent review findings, address every one, or say why one is wrong.
  reviewer:
    description: Reviews the implementer's branch against the goal
    grants: [github:read]        # it can read the work but not change it
    fresh: true                  # a new reviewer each round, not anchored on the last
    # model: opus                # a reviewer can run on a different model
    prompt: |
      Review the work on this branch against this goal: {{goal}}

      Read the diff against the default branch and run the tests. Look for behaviour that is
      wrong or missing, not for style. Request changes only for what must be fixed before
      this ships, and say exactly where and what.
loop:
  implement: implementer
  review: reviewer
  max_rounds: 4
prompt: |
  Goal: {{goal}}

  Run the loop until the reviewer approves. Then ask the implementer to open a pull request,
  and report the link with a one-paragraph summary of what was built and what review changed.
```

> [!TIP]
> Give the goal a test of success the reviewer can check, such as a command that must pass. A reviewer with nothing to measure against approves too easily or never.

## Permissions

The `grants` list is a ceiling, not a gift. The agents get the grants listed, intersected with what the person running the workflow holds. A workflow can’t give anyone more than they already have.

## Run it

From **New work**, choose **Run a workflow**, pick it, and fill in its inputs. From the command line:

```bash
hyperbeam workflow ship-feature -i feature="CSV export" --follow
```

Another example, a read-only investigation:

```yaml title="triage-bug.yaml"
# A single agent that investigates and reports, without changing anything.
name: triage-bug
description: Reproduce a bug, find the cause and write up what a fix would involve
role: worker
repos:
  - acme/web
grants:
  - github:read
inputs:
  - name: report
    description: What is going wrong, and how to reproduce it if known
prompt: |
  Investigate this bug report: {{report}}

  Reproduce it if you can, find the cause, and report: what is wrong, where, how sure you are,
  and what a fix would involve. Do not change code.
```
