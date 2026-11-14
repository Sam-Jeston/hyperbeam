# Hyperbeam

*Because it’s the best move.*

**A self-hosted platform where a team's coding agents work in the open.**

Today most agent work happens in someone's terminal and stays invisible until a pull request appears.
Hyperbeam moves that work one level up: anyone on the team can describe an outcome, an agent picks it up in
its own sandbox, and everyone can watch the scoping and the building as it happens.

- **A shared board.** Workstreams move from scoping to building to review. Each shows the agents on it,
  what they are doing right now, and the pull requests they produced.
- **Any harness.** Claude Code, Codex, Gemini CLI, OpenCode, or anything else that speaks the
  [Agent Client Protocol](https://agentclientprotocol.com). Bring a team API key or your own credential.
- **Permissions that belong to people.** An agent can do only what the person who started it may:
  GitHub or GitLab (read, write, open PR, merge PR) and each connected MCP service (read, write), per repository.
- **Agents never hold your credentials.** GitHub, GitLab and connected MCP services (Linear, Notion, Slack, Datadog, Sentry and more, or your own) are reached through Hyperbeam's tool
  gateway and git proxy, which check every request and record it.
- **From Slack and Discord.** Mention the bot to start an agent, reply in the thread to direct it, and
  answer its questions with a button. Each person acts as their own Hyperbeam account.
- **A conductor.** One agent that scopes a request, starts worker agents across repositories, and is
  told when each reports back.
- **Skills, workflows and team notes.** What your team teaches its agents, kept in one place and
  shared by every session.

## How it fits together

```
 Browser / CLI / Slack / Discord
      │  REST + server-sent events (chat apps: the server dials out)
┌─────▼────────────────── hyperbeam-server ────────────────────────┐
│ API · teams and permissions · session manager · event log      │
│ tool gateway (MCP) · git proxy · egress allowlist proxy        │
└─────┬──────────────────────────────────┬───────────────────────┘
      │ Postgres (all state)             │ one pod per live session
┌─────▼──────────────── agent sandbox ───────────────────────────┐
│ hyperbeam-runner ⇄ ACP over stdio ⇄ harness (Claude Code, …)     │
│ may reach: the control plane. Nothing else, except through it. │
└────────────────────────────────────────────────────────────────┘
```

A session's sandbox holds exactly one credential: a run token that identifies the session. With it the
runner dials out to the control plane; nothing connects in. That token is what the sandbox presents to:

| Path | What it is for | What is checked |
|---|---|---|
| `/git/…` | `git clone`, `fetch`, `push` | repository is one of the session's; `github:read` to fetch, `github:write` to push, `github:merge_pr` to push to the default branch (`gitlab:…` on GitLab) |
| `/mcp` | tools for GitHub, GitLab, connected MCP servers and the platform | the grant each tool needs, the repository, and for connections that an admin cleared the tool |
| `/mcp` `web_fetch` | reading public web pages | `web:read`; the URL is one a person gave the agent, a search returned or a fetched page links to; the address is public |
| egress proxy | everything else on the network | host is on the allowlist |

The token is revoked the moment the session ends. Grants are evaluated against what the session's owner
holds *now*, so removing a permission from a person takes effect on their running agents immediately.

## Quick start on a laptop

This runs agents as ordinary local processes with **no isolation**. It is for trying Hyperbeam out.

```sh
docker run -d --name hyperbeam-pg -e POSTGRES_USER=hyperbeam -e POSTGRES_PASSWORD=hyperbeam \
  -e POSTGRES_DB=hyperbeam -p 127.0.0.1:5440:5432 postgres:16

(cd web && bun install && bun run build)      # the UI is embedded in the server binary
cargo build --release

export HYPERBEAM_DATABASE_URL=postgres://hyperbeam:hyperbeam@127.0.0.1:5440/hyperbeam
export HYPERBEAM_MASTER_KEY=$(./target/release/hyperbeam-server gen-key)   # keep it: it encrypts stored credentials
./target/release/hyperbeam-server serve
```

Open <http://localhost:8080>. The first person in creates the team and owns it.

Then connect an agent. In this mode it runs on your machine, so install its ACP adapter, for example
for Claude Code:

```sh
npm i -g @agentclientprotocol/claude-agent-acp
```

Add a credential for it under *Settings → Harnesses*, then press <kbd>C</kbd> (or *New work*), describe
the task and start the agent.

## Run in a local cluster

The quick start is for trying Hyperbeam out and for working on it. To keep it running on your own
machine, run the Helm chart in a local [kind](https://kind.sigs.k8s.io) cluster (0.24 or later, whose
network plugin enforces NetworkPolicy). Each session then gets its own sandbox pod, the control plane is
restarted if it crashes, and Postgres keeps its data on a persistent volume. You need Docker, kind,
kubectl and Helm.

```sh
kind create cluster --name hyperbeam

docker build -f deploy/docker/Dockerfile --target server -t hyperbeam/server:dev .
docker build -f deploy/docker/Dockerfile --target runner -t hyperbeam/runner:dev .
docker build -f deploy/docker/Dockerfile.harness --target claude-code -t hyperbeam/harness-claude-code:dev .
kind load docker-image --name hyperbeam \
  hyperbeam/server:dev hyperbeam/runner:dev hyperbeam/harness-claude-code:dev

openssl rand -base64 32 > hyperbeam-master-key     # keep it safe: it encrypts stored credentials
helm install hyperbeam deploy/helm/hyperbeam --namespace hyperbeam --create-namespace \
  --set masterKey="$(cat hyperbeam-master-key)" \
  --set image.repository=hyperbeam/server --set image.tag=dev \
  --set agents.runnerImage.repository=hyperbeam/runner --set agents.runnerImage.tag=dev
kubectl -n hyperbeam rollout status deploy/hyperbeam
kubectl -n hyperbeam port-forward svc/hyperbeam 8080:8080
```

Open <http://localhost:8080> and create the team. Under *Settings → Harnesses → Claude Code → Edit*, set
the container image to `hyperbeam/harness-claude-code:dev` and add a credential. No ACP adapter is needed
on your machine: it is in the harness image.

To update, rebuild with a new tag, `kind load` it, and run
`helm upgrade hyperbeam deploy/helm/hyperbeam -n hyperbeam --reuse-values --set image.tag=… --set agents.runnerImage.tag=…`.
Data survives restarts of the control plane, Docker and your machine, but `kind delete cluster` deletes
it. The docs page *Run in a local cluster* covers backups, an external Postgres, and k3d and minikube.

## Install on Kubernetes

The images are published to `ghcr.io/sam-jeston/hyperbeam/` for amd64 and arm64 on every push to
`main` (`server`, `runner`, and a `harness-…` image per coding agent). The chart and the built-in
harness profiles use them, so there is nothing to build:

```sh
helm install hyperbeam deploy/helm/hyperbeam --namespace hyperbeam --create-namespace \
  --set masterKey="$(openssl rand -base64 32)" \
  --set publicUrl=https://hyperbeam.example.com
```

To run images you built yourself, see *Install on Kubernetes* in the docs.

What the chart creates:

- the control plane (one replica) and, unless you set `database.url`, a single-instance Postgres;
- a namespace for sandboxes (`hyperbeam-agents`), under the `restricted` Pod Security Standard, where the
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

1. **Connect GitHub or GitLab** (*Settings → Integrations*). On GitHub, use a GitHub App for an organisation: Hyperbeam then
   mints a short-lived token per repository per request. A fine-grained access token works for one person.
2. **Register repositories** (*Settings → Repositories*). Agents can reach only these.
3. **Give a harness a credential** (*Settings → Harnesses*): a team API key, or each person's own.
4. **Invite people** (*People & permissions*) and tick what each person's agents may do.
   Hyperbeam sends no email; you pass the invite link on.
5. Optionally connect services such as Linear, Notion, Slack, Datadog and Sentry, or your own MCP servers, and add skills and workflows in the *Library*.

### Permissions

| Grant | Lets an agent |
|---|---|
| `github:read` | clone registered repositories; read issues and pull requests |
| `github:write` | push branches (not the default branch); comment; create issues |
| `github:open_pr` | open pull requests |
| `github:merge_pr` | merge pull requests; push to the default branch |
| `gitlab:read` … `gitlab:merge_pr` | the same four on GitLab, where pull requests are merge requests |
| `<connection>:read` / `<connection>:write` | use a connection's read / write tools, e.g. `linear:write`, `notion:read` |

A session's grants are fixed at launch to what its owner holds, optionally narrowed, and are narrowed
further if the owner later loses a grant. A worker started by a conductor can hold at most what the
conductor holds. Everyone on the team can watch any session; only its owner, an admin, or someone who
holds at least the same grants can send it messages, because prompting an agent means acting with its
owner's permissions.

### Models

Each session runs on a model you choose when starting it (*New work*, `hyperbeam run --model`, or
`model:` in a workflow). A harness profile can set the default used when nobody chooses, and can
limit the choice to a list (*Settings → Harnesses → Edit*). A conductor's workers inherit its model
unless it picks another for a piece of work.

The list offered comes from the harness itself: Hyperbeam reads the standard ACP `model` session
option and remembers what each harness reported. It asks as soon as it can: when a harness is
given a credential, or its image or command changes, Hyperbeam starts the harness for a few seconds
in a sandbox of its own, records the list and removes the sandbox, so the picker is correct before
anyone's first session. *Refresh models* under *Settings → Harnesses* asks again. That sandbox is
not a session: it has no repositories, tools or permissions. A model the harness does not recognise does not fail the session; it runs on the harness
default and says so in its log. For agents that take their model from the environment instead, a
profile can name a variable that also receives the choice.

### Subscription credentials

Harness profiles can accept a personal subscription token (for Claude Code,
`claude setup-token`) as well as an API key. Hyperbeam treats these as personal: they can only be set by
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
hyperbeam login --server https://hyperbeam.example.com
hyperbeam run "Fix the flaky checkout test" --repo acme/web --model sonnet --follow
hyperbeam run "Add dark mode" --conductor --repo acme/api --repo acme/web --workstream "Dark mode"
hyperbeam workflow ship-feature -i feature="CSV export" --follow
hyperbeam ls
hyperbeam logs -f 3ff6c49b
hyperbeam send 3ff6c49b "Also update the changelog" --follow
hyperbeam stop 3ff6c49b
```

`HYPERBEAM_SERVER` and `HYPERBEAM_API_KEY` in the environment take the place of `hyperbeam login`.

## Configuration

`hyperbeam-server serve --help` lists every option. The ones that matter most:

| Variable | Meaning |
|---|---|
| `HYPERBEAM_DATABASE_URL` | Postgres connection URL |
| `HYPERBEAM_MASTER_KEY` | 32 random bytes, base64; encrypts stored credentials |
| `HYPERBEAM_MASTER_KEY_AWS_SECRET` / `HYPERBEAM_MASTER_KEY_AWS_KMS_KEY` | instead of the above: read the master key from AWS Secrets Manager, or keep it wrapped under an AWS KMS key |
| `HYPERBEAM_PUBLIC_URL` | where people reach the UI |
| `HYPERBEAM_INTERNAL_URL` | where sandboxes reach the server |
| `HYPERBEAM_SANDBOX_DRIVER` | `process` (development) or `kubernetes` |
| `HYPERBEAM_EGRESS_LISTEN` / `HYPERBEAM_EGRESS_ADDR` | enable the egress proxy and tell sandboxes where it is |
| `HYPERBEAM_EGRESS_ALLOW` | hosts every sandbox may reach |
| `HYPERBEAM_IDLE_TIMEOUT_SECS`, `HYPERBEAM_MAX_SESSIONS` | sandbox lifetime and concurrency |

To change the master key of an installation that holds credentials, run `hyperbeam-server rotate-master-key`
(it re-encrypts them under the new key, and can move between the three sources), then restart the server.

### Adding a harness

Any agent that speaks ACP on stdio can be added under *Settings → Harnesses*: a container image with
`git` and the agent, the command that starts it, the environment variable its credential goes in, and
the hosts it needs to reach. `deploy/docker/Dockerfile.harness` is a starting point.

### Environments

To give agents your language toolchains, add an environment under *Settings → Environments*: any
glibc-based image with `git`, such as `rust:1.90` or the image your CI builds in. Assign it to a
repository and agents working there run in it, with the coding agent carried in from the harness
image. A repository can also have a setup command (`cargo fetch`, `bun install`) that runs in each
fresh checkout before the agent starts.

## Development

```sh
cargo build -p hyperbeam-runner -p hyperbeam-test-agent
HYPERBEAM_TEST_DATABASE_URL=postgres://hyperbeam:hyperbeam@127.0.0.1:5440/postgres cargo test --workspace
(cd web && bun run typecheck && bun run build)
```

The end-to-end tests start a real server, real runner processes driving a scripted ACP agent
(`crates/test-agent`, not shipped), and a fake GitHub and GitLab that serve the git
smart-HTTP protocol, and need Postgres; without `HYPERBEAM_TEST_DATABASE_URL` they are skipped.

| Path | Contents |
|---|---|
| `crates/core` | permissions, runner protocol, workflow format |
| `crates/server` | API, session manager, tool gateway, git proxy, egress proxy, sandbox drivers |
| `crates/runner` | in-sandbox runner; ACP client |
| `crates/test-agent` | scripted ACP agent for the end-to-end tests (not shipped) |
| `crates/cli` | the `hyperbeam` command |
| `web` | the UI (React, built with Bun) |
| `site` | the landing page, rendered to static HTML (`bun run dev` in `site/`) |
| `docs` | the documentation: Markdown in `docs/content/`, rendered to static HTML |
| `brand` | brand tokens and guidelines shared by `web`, `site` and `docs` |
| `deploy` | Dockerfiles and the Helm chart |

## Testing and limits

Covered by automated tests: the permission model; sign-up, invites and roles; the full session
lifecycle through the runner and ACP; cloning and pushing through the git
proxy, including default-branch protection; opening pull requests through the gateway; denial and
audit of what is not permitted; immediate revocation; resume after teardown; conductor delegation;
model selection; and the egress allowlist.

Verified by hand on a local `kind` cluster (Kubernetes 1.34) with the Helm chart: the control plane
and bundled Postgres come up; a session runs in its own pod as a non-root user with no service account
token; allowlisted hosts are reachable through the egress proxy and others are refused, both through
the proxy and directly; a live session survives a control plane restart; stopping a session removes
its pod and token secret.

Known limits: one team per installation; the control plane runs as a single replica; sign-in is email
and password only (no OIDC yet); a connection has one credential or OAuth sign-in, so every agent acts as that
one account; workspaces are ephemeral, so work survives a teardown only if pushed.

## Licence

Apache-2.0. See [LICENSE](LICENSE).
