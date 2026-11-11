# Noemune

**A self-hosted platform where a team's coding agents work in the open.**

Today most agent work happens in someone's terminal and stays invisible until a pull request appears.
Noemune moves that work one level up: anyone on the team can describe an outcome, an agent picks it up in
its own sandbox, and everyone can watch the scoping and the building as it happens.

- **A shared board.** Workstreams move from scoping to building to review. Each shows the agents on it,
  what they are doing right now, and the pull requests they produced.
- **Any harness.** Claude Code, Codex, Gemini CLI, OpenCode, or anything else that speaks the
  [Agent Client Protocol](https://agentclientprotocol.com). Bring a team API key or your own credential.
- **Permissions that belong to people.** An agent can do only what the person who started it may:
  GitHub (read, write, open PR, merge PR), Linear (read, write), Notion (read, write), per repository.
- **Agents never hold your credentials.** GitHub, Linear and Notion are reached through Noemune's tool
  gateway and git proxy, which check every request and record it.
- **A conductor.** One agent that scopes a request, starts worker agents across repositories, and is
  told when each reports back.
- **Skills, workflows and team notes.** What your team teaches its agents, kept in one place and
  shared by every session.

> Status: early. The control plane, runner, UI and CLI work and are covered by end-to-end tests using a
> scripted mock harness and a fake GitHub. See [What is and is not verified](#what-is-and-is-not-verified).

## How it fits together

```
 Browser / CLI
      │  REST + server-sent events
┌─────▼────────────────── noemune-server ────────────────────────┐
│ API · teams and permissions · session manager · event log      │
│ tool gateway (MCP) · git proxy · egress allowlist proxy        │
└─────┬──────────────────────────────────┬───────────────────────┘
      │ Postgres (all state)             │ one pod per live session
┌─────▼──────────────── agent sandbox ───────────────────────────┐
│ noemune-runner ⇄ ACP over stdio ⇄ harness (Claude Code, …)     │
│ may reach: the control plane. Nothing else, except through it. │
└────────────────────────────────────────────────────────────────┘
```

A session's sandbox holds exactly one credential: a run token that identifies the session. With it the
runner dials out to the control plane; nothing connects in. That token is what the sandbox presents to:

| Path | What it is for | What is checked |
|---|---|---|
| `/git/…` | `git clone`, `fetch`, `push` | repository is one of the session's; `github:read` to fetch, `github:write` to push, `github:merge_pr` to push to the default branch |
| `/mcp` | tools for GitHub, Linear, Notion and the platform | the grant each tool needs, and the repository |
| egress proxy | everything else on the network | host is on the allowlist |

The token is revoked the moment the session ends. Grants are evaluated against what the session's owner
holds *now*, so removing a permission from a person takes effect on their running agents immediately.

## Quick start on a laptop

This runs agents as ordinary local processes with **no isolation**. It is for trying Noemune out.

```sh
docker run -d --name noemune-pg -e POSTGRES_USER=noemune -e POSTGRES_PASSWORD=noemune \
  -e POSTGRES_DB=noemune -p 127.0.0.1:5440:5432 postgres:16

(cd web && bun install && bun run build)      # the UI is embedded in the server binary
cargo build --release

export NOEMUNE_DATABASE_URL=postgres://noemune:noemune@127.0.0.1:5440/noemune
export NOEMUNE_MASTER_KEY=$(./target/release/noemune-server gen-key)   # keep it: it encrypts stored credentials
./target/release/noemune-server serve
```

Open <http://localhost:8080>. The first person in creates the team and owns it.

To see it work before adding a model key, start an agent with the **Mock harness** (under *Harness, skills
and permissions* in the *New work* dialog). It runs a script instead of a model:

```
!plan look around; report
!sh ls -la
!say Nothing here yet.
```

For a real agent in this mode, install its ACP adapter on your machine
(for example `npm i -g @agentclientprotocol/claude-agent-acp`) and add a credential under
*Settings → Harnesses*.

## Install on Kubernetes

Build and push the images (the project does not publish images yet):

```sh
docker build -f deploy/docker/Dockerfile --target server -t REGISTRY/noemune/server:0.1.0 .
docker build -f deploy/docker/Dockerfile --target runner -t REGISTRY/noemune/runner:0.1.0 .
docker build -f deploy/docker/Dockerfile.harness --target claude-code -t REGISTRY/noemune/harness-claude-code:0.1.0 .
```

Install the chart:

```sh
helm install noemune deploy/helm/noemune --namespace noemune --create-namespace \
  --set masterKey="$(openssl rand -base64 32)" \
  --set image.repository=REGISTRY/noemune/server \
  --set agents.runnerImage.repository=REGISTRY/noemune/runner \
  --set publicUrl=https://noemune.example.com
```

Then, in the UI, set each harness profile's image under *Settings → Harnesses* to the one you pushed.

What the chart creates:

- the control plane (one replica) and, unless you set `database.url`, a single-instance Postgres;
- a namespace for sandboxes (`noemune-agents`), under the `restricted` Pod Security Standard, where the
  control plane may create and delete pods and their token secrets, and nothing else;
- a NetworkPolicy that lets sandboxes reach only the control plane and cluster DNS. With
  `egress.enforce: true` (the default) everything else goes through the allowlist proxy.
  **This relies on a CNI that enforces NetworkPolicy** (Calico, Cilium and others); on a cluster without
  one the policy is silently ignored. Some CNIs, including the one `kind` ships, do not apply policy to
  traffic from a pod to its own node, so a sandbox may still be able to open a connection to the
  Kubernetes API server. It has no service account token, so it is an anonymous client there; if your
  cluster grants anonymous users anything, tighten that or add a CNI-specific host policy.

Each session gets a pod that runs as a non-root user with no service account token, no added
capabilities, and resource limits. Idle sandboxes are torn down after 15 minutes; the session can be
resumed with a message, in a fresh sandbox, from the branch it pushed.

## Setting up a team

1. **Connect GitHub** (*Settings → Integrations*). Use a GitHub App for an organisation: Noemune then
   mints a short-lived token per repository per request. A fine-grained access token works for one person.
2. **Register repositories** (*Settings → Repositories*). Agents can reach only these.
3. **Give a harness a credential** (*Settings → Harnesses*): a team API key, or each person's own.
4. **Invite people** (*People & permissions*) and tick what each person's agents may do.
   Noemune sends no email; you pass the invite link on.
5. Optionally connect Linear and Notion, and add skills and workflows in the *Library*.

### Permissions

| Grant | Lets an agent |
|---|---|
| `github:read` | clone registered repositories; read issues and pull requests |
| `github:write` | push branches (not the default branch); comment; create issues |
| `github:open_pr` | open pull requests |
| `github:merge_pr` | merge pull requests; push to the default branch |
| `linear:read` / `linear:write` | read / create, update and comment on issues |
| `notion:read` / `notion:write` | read / create and append to pages |

A session's grants are fixed at launch to what its owner holds, optionally narrowed, and are narrowed
further if the owner later loses a grant. A worker started by a conductor can hold at most what the
conductor holds. Everyone on the team can watch any session; only its owner, an admin, or someone who
holds at least the same grants can send it messages, because prompting an agent means acting with its
owner's permissions.

### Models

Each session runs on a model you choose when starting it (*New work*, `noemune run --model`, or
`model:` in a workflow). A harness profile can set the default used when nobody chooses, and can
limit the choice to a list (*Settings → Harnesses → Edit*). A conductor's workers inherit its model
unless it picks another for a piece of work.

The list offered comes from the harness itself: Noemune reads the standard ACP `model` session
option and remembers what each harness reported. It asks as soon as it can: when a harness is
given a credential, or its image or command changes, Noemune starts the harness for a few seconds
in a sandbox of its own, records the list and removes the sandbox, so the picker is correct before
anyone's first session. *Refresh models* under *Settings → Harnesses* asks again. That sandbox is
not a session: it has no repositories, tools or permissions. A model the harness does not recognise does not fail the session; it runs on the harness
default and says so in its log. For agents that take their model from the environment instead, a
profile can name a variable that also receives the choice.

### Subscription credentials

Harness profiles can accept a personal subscription token (for Claude Code,
`claude setup-token`) as well as an API key. Noemune treats these as personal: they can only be set by
their owner for their own sessions, never as a team credential. Whether a subscription may be used
this way is decided by the provider's terms, which changed more than once during 2026. For a team or
anything automated, use API keys.

### Workflows

A workflow is a saved, parameterised launch, written in YAML:

```yaml
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

Skills use the open `SKILL.md` format and are installed into each sandbox at `.claude/skills/` and
`.agents/skills/`.

## Command line

Create an API key under *Settings → My account*, then:

```sh
noemune login --server https://noemune.example.com
noemune run "Fix the flaky checkout test" --repo acme/web --model sonnet --follow
noemune run "Add dark mode" --conductor --repo acme/api --repo acme/web --workstream "Dark mode"
noemune workflow ship-feature -i feature="CSV export" --follow
noemune ls
noemune logs -f 3ff6c49b
noemune send 3ff6c49b "Also update the changelog" --follow
noemune stop 3ff6c49b
```

`NOEMUNE_SERVER` and `NOEMUNE_API_KEY` in the environment take the place of `noemune login`.

## Configuration

`noemune-server serve --help` lists every option. The ones that matter most:

| Variable | Meaning |
|---|---|
| `NOEMUNE_DATABASE_URL` | Postgres connection URL |
| `NOEMUNE_MASTER_KEY` | 32 random bytes, base64; encrypts stored credentials |
| `NOEMUNE_PUBLIC_URL` | where people reach the UI |
| `NOEMUNE_INTERNAL_URL` | where sandboxes reach the server |
| `NOEMUNE_SANDBOX_DRIVER` | `process` (development) or `kubernetes` |
| `NOEMUNE_EGRESS_LISTEN` / `NOEMUNE_EGRESS_ADDR` | enable the egress proxy and tell sandboxes where it is |
| `NOEMUNE_EGRESS_ALLOW` | hosts every sandbox may reach |
| `NOEMUNE_IDLE_TIMEOUT_SECS`, `NOEMUNE_MAX_SESSIONS` | sandbox lifetime and concurrency |

### Adding a harness

Any agent that speaks ACP on stdio can be added under *Settings → Harnesses*: a container image with
`git` and the agent, the command that starts it, the environment variable its credential goes in, and
the hosts it needs to reach. `deploy/docker/Dockerfile.harness` is a starting point; add your language
toolchains to it so agents can build and test your code.

## Development

```sh
cargo build -p noemune-runner
NOEMUNE_TEST_DATABASE_URL=postgres://noemune:noemune@127.0.0.1:5440/postgres cargo test --workspace
(cd web && bun run typecheck && bun run build)
```

The end-to-end tests start a real server, real runner processes and a fake GitHub that serves the git
smart-HTTP protocol, and need Postgres; without `NOEMUNE_TEST_DATABASE_URL` they are skipped.

| Path | Contents |
|---|---|
| `crates/core` | permissions, runner protocol, workflow format |
| `crates/server` | API, session manager, tool gateway, git proxy, egress proxy, sandbox drivers |
| `crates/runner` | in-sandbox runner; ACP client; mock harness |
| `crates/cli` | the `noemune` command |
| `web` | the UI (React, built with Bun) |
| `deploy` | Dockerfiles and the Helm chart |

## What is and is not verified

Verified by automated tests: the permission model; sign-up, invites and roles; the full session
lifecycle through the runner and ACP (with the mock harness); cloning and pushing through the git
proxy, including default-branch protection; opening pull requests through the gateway; denial and
audit of what is not permitted; immediate revocation; resume after teardown; conductor delegation;
model selection; and the egress allowlist. The UI was exercised in a browser against the mock harness.

Verified by hand on a local `kind` cluster (Kubernetes 1.34) with the Helm chart: the control plane
and bundled Postgres come up; a session runs in its own pod as a non-root user with no service account
token; allowlisted hosts are reachable through the egress proxy and others are refused, both through
the proxy and directly; a live session survives a control plane restart; stopping a session removes
its pod and token secret.

Not yet verified, and likely to need fixes on first contact:

- **A real harness.** The ACP client follows the published v1 schema but has only driven the mock.
- **Linear, Notion and GitHub App mode.** Written against the documented APIs; never called against
  the real services.
- **The harness images.** `Dockerfile.harness` has not been built for each agent.

Known limits: one team per installation; the control plane runs as a single replica; sign-in is email
and password only (no OIDC yet); Linear and Notion connect with a key rather than an OAuth flow, so
agents act as that key's user; workspaces are ephemeral, so work survives a teardown only if pushed.

## Licence

Apache-2.0. See [LICENSE](LICENSE).
