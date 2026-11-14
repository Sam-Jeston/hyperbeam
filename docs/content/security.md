---
title: Security model
description: What a sandbox holds, what it can reach, and how every action outside it is checked and recorded.
---

Hyperbeam is built so that an agent can do only what the person who started it may, and so that agents never hold your integration credentials.

## What a sandbox holds

A session’s sandbox holds exactly one Hyperbeam credential: a **run token** that identifies the session. With it the runner dials out to the control plane; nothing connects in. The token is revoked the moment the session ends.

The sandbox also receives the [harness credential](/credentials#what-the-agent-can-see) its agent needs to call its model provider.

## Every way out goes through Hyperbeam

| Path | What it is for | What is checked |
|---|---|---|
| `/git/…` | `git clone`, `fetch`, `push` | The repository is one of the session’s. `github:read` to fetch, `github:write` to push, `github:merge_pr` to push to the default branch (`gitlab:…` for a repository on GitLab). |
| `/mcp` | Tools for GitHub, GitLab, connected MCP servers and the platform | The grant each tool needs, and the repository. For a connection, that an admin has cleared the tool. |
| `/mcp` `web_fetch` | Reading public web pages | The session holds `web:read`, the URL is one the agent may fetch, and the address is on the public internet. See [Web access](#web-access). |
| Egress proxy | Everything else on the network | The host is on the allowlist. |

GitHub, GitLab and connection credentials, including the tokens from an OAuth sign-in, stay in the control plane. The git proxy and tool gateway add them to requests they have checked; the agent never sees them. Connected MCP servers are called by the control plane, not the sandbox, so they need no place on the egress allowlist.

A custom connection’s tools are hidden until an admin approves each one, and hidden again if the server changes one afterwards. See [Custom connections](/integrations#custom-connections).

## Grants are checked as they are now

Grants are evaluated against what the session’s owner holds **now**, not only at launch. Removing a permission from a person takes effect on their running agents immediately. See [People and permissions](/permissions).

## Network egress

On Kubernetes, a NetworkPolicy lets sandboxes reach only the control plane and cluster DNS. With the egress proxy enabled, everything else goes through an allowlist: package registries by default, plus hosts admins add under **Settings → General**, plus each harness profile’s own hosts. Requests to other hosts are refused and recorded.

The NetworkPolicy relies on a CNI that enforces it. See [Network isolation](/install-kubernetes#network-isolation).

## Web access

A sandbox cannot browse the web: the egress allowlist covers package registries and the model provider, nothing more. Agents read pages through the gateway’s `web_fetch` tool instead. The control plane fetches the page and returns it as markdown, so each request is checked, recorded in the audit log and shown in the session.

A sandbox holds your source code, and a page an agent reads can try to instruct it. An agent that could fetch any URL could be talked into putting what it holds into one. So by default an agent can fetch only URLs it did not write itself:

- URLs a person wrote: in a message to the agent, the workstream’s brief, or a skill it was given.
- URLs a web search returned, when the search runs through a [web search connection](/integrations#web-search).
- Links on pages it has already fetched.
- Any page on a site an admin has listed under **Settings → General**.

A conductor and its workers share what they may fetch. A URL a conductor writes to a worker does not count.

Admins choose between this, **any public page**, **only the listed sites**, and **none**. An agent needs the `web:read` grant in every case. With **any public page**, an agent can fetch any URL on the public internet, including one it wrote. That removes the protection above: a page that instructs an agent can get it to send what it holds to any site in a URL. The other checks below still apply, and every fetch is still recorded.

`web_fetch` makes `GET` requests only, sends no credentials, and refuses URLs that carry a username or password. It refuses any address that is not on the public internet (loopback, private networks, link-local and cloud metadata addresses), checks every redirect the same way, and pins each request to the address it checked. It reads at most 2 MB of a text response, and a session can fetch 30 pages a minute.

What this does not stop:

- **Listed sites.** On a site you list, an agent can fetch any URL, including one it wrote. List sites you would trust with a query string.
- **Slow leaks.** Which of a page’s links an agent chooses to follow is visible to that site. The rate limit and the audit log bound this; they do not remove it.
- **The harness’s own search.** Claude Code, Codex and Gemini CLI run their built-in web search on the model provider’s side. Hyperbeam does not see those searches, and URLs they return are not ones `web_fetch` will follow. Use a web search connection for that.

## The sandbox itself

Each Kubernetes sandbox is a pod that runs as a non-root user, with no service account token and no added capabilities, under the `restricted` Pod Security Standard, with CPU, memory and disk limits. The control plane’s own permissions in the sandbox namespace are limited to creating and deleting pods and their token secrets, and reading events. Besides sandboxes, the only pods it creates are the short-lived ones that pull an image when an admin asks from the Cluster page. They run the same images under the same restrictions, do nothing but start, and have no network access at all. Outside it, it may list the cluster’s nodes, for the [Cluster page](/install-kubernetes#see-how-the-cluster-is-doing), unless you turn that off.

## The audit log

Every attempt by an agent to act outside its sandbox is recorded: tool calls, git operations and blocked network requests, with the session, the person it acted for, and whether it was allowed. Admins see it under **Settings → Audit log**.

## Stored credentials

Credentials are encrypted at rest with AES-256-GCM under the installation’s master key, each bound to the record it belongs to. The master key can be supplied directly, read from AWS Secrets Manager, or kept encrypted by an AWS KMS key so that it exists unencrypted only in the server’s memory, and it can be [rotated](/credentials#rotating-the-master-key). See [Credentials](/credentials#the-master-key). People’s passwords are hashed with Argon2, and API keys and sign-in tokens are stored only as hashes.

## Known limits

- One team per installation, and the control plane runs as a single replica.
- Sign-in is email and password only; there is no OIDC yet.
- A connection has one credential or one OAuth sign-in, shared by every agent, so agents act as that one account rather than as the person who started them.
