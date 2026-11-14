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
| `prompt` | yes | The request. `{{name}}` is replaced by the input of that name. |

Each input has a `name`, an optional `description`, `required` (true unless set to false) and an optional `default`.

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
