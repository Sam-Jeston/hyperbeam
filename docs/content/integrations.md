---
title: Integrations
description: Connect GitHub or GitLab, first-class services like Linear, Notion, Slack, Datadog, Sentry and web search, and any other MCP server.
---

Integrations are set under **Settings → Integrations** by an admin. Their credentials stay in Hyperbeam’s control plane. Agents never see them: every request an agent makes goes through Hyperbeam, which checks it against the permissions of the person who started the agent. Select **Test** on a connected integration to check its credential.

## GitHub

Choose how Hyperbeam authenticates:

- **GitHub App** (recommended for organisations). Create a GitHub App with repository permissions *Contents*, *Pull requests* and *Issues* set to read & write, and install it on the repositories you will register. Paste its App ID and a generated private key. Hyperbeam mints a short-lived token per repository for each request.
- **Access token** (simplest; good for one person or a small team). A fine-grained personal access token limited to the repositories you will register, with *Contents*, *Pull requests* and *Issues* set to read & write. Everything agents do on GitHub appears as this token’s user.

## GitLab

Works with gitlab.com and with a GitLab your team runs itself. Give Hyperbeam:

- the **GitLab address**, if it is not `https://gitlab.com`, and
- an **access token** with the `api` and `write_repository` scopes. A project or group access token limited to what you will register is best; a personal access token works too. Give it at least the Developer role, or Maintainer if agents should merge or push to protected branches. Everything agents do on GitLab appears as this token’s user.

GitLab has its own permissions (`gitlab:read`, `gitlab:write`, `gitlab:open_pr`, `gitlab:merge_pr`), which can be granted once it is connected. Nobody holds them to begin with, and disconnecting GitLab removes them from everyone. Agents get `gitlab_*` tools for issues and merge requests in place of the `github_*` ones.

GitHub and GitLab can be connected at the same time, and one agent can work across repositories on both.

## Repositories

Agents can reach only repositories registered under **Settings → Repositories**, and only the ones their owner [has access to](/permissions#repository-access). Register one as `owner/name`, or for a GitLab project under subgroups, its whole path: `group/subgroup/project`. Pasting the repository’s address works too. With both hosts connected, choose which one the repository is on.

A path can be registered once, on one host: `acme/api` cannot be both a GitHub repository and a GitLab project.

Removing a repository stops new agents from using it. Agents already running keep the repositories they started with until they stop. The repository itself is untouched.

## Connected services

Everything else is a **connection**: an MCP server that agents reach through Hyperbeam’s tool gateway. Hyperbeam calls the server from the control plane with the connection’s credential, so agents never see the credential or reach the server directly. A connection’s tools appear to agents as `<name>_<tool>`, for example `sentry_search_issues`.

Each of a connection’s tools is a **read** tool, a **write** tool, or **hidden**. Agents use read tools with `<name>:read` and write tools with `<name>:write`, which you grant per person under [People & permissions](/permissions). A new connection’s permissions are granted to nobody, its creator included, until you tick them.

Select **Tools** on a connection to see what it offers and change what each tool is for. Your choice for a tool overrides whatever Hyperbeam decided automatically.

### First-class services

These connect with one credential or one sign-in, and Hyperbeam sorts their tools into read and write for you.

| Service | Credential | How tools are sorted |
|---|---|---|
| **Better Stack** | A Better Stack API token (*Settings → API tokens*). A token without *Read & write* access limits every agent to reading. | By tool name: `list_`, `get_` and `search_` tools are read tools, and `create_`, `update_`, `remove_` and `add_` tools, among others, are write tools. Better Stack does not publish its tool names, so expect some to wait for you. |
| **Datadog** | None: select **Sign in with Datadog**. Choose your Datadog site, and optionally name the toolsets to offer; Datadog offers only `core` otherwise. Reading needs Datadog’s *MCP Read* permission, and changing things *MCP Write*. GovCloud is not supported. | By tool name: `search_`, `get_`, `list_`, `aggregate_` and `analyze_` tools, among others, are read tools, and `create_`, `update_`, `delete_`, `edit_`, `upsert_` and `execute_` tools, among others, are write tools. Anything else waits for you. |
| **Figma** | None: select **Sign in with Figma**. Figma admits only MCP clients in its [MCP catalog](https://www.figma.com/mcp-catalog), so the sign-in is refused until Figma has approved your client. | By tool name: `get_`, `list_`, `search_` and `download_` tools are read tools, and `create_`, `generate_`, `update_`, `upload_` and `use_figma`, among others, are write tools. |
| **Granola** | None: select **Sign in with Granola**. Every agent sees the meetings of the person who signs in. Transcripts and editing need a paid Granola plan. | By tool name: `search_`, `list_`, `get_` and `query_` tools are read tools, and `create_`, `update_`, `add_`, `remove_` and `generate_` tools, with `switch_workspace` and `sign_out`, are write tools. |
| **HubSpot** | An OAuth client you create in HubSpot (*Development → MCP Connectors → Create MCP connector*) with Hyperbeam’s redirect URL. Enter its client ID and secret, then sign in. See [Signing in with OAuth](#signing-in-with-oauth). | By tool name: `get_`, `search_`, `read_` and `query_` tools are read tools, and `manage_` tools and `submit_feedback` are write tools. |
| **Linear** | A Linear API key (*Settings → Security & access → Personal API keys*). A key with only the Read permission limits every agent to reading. | By Linear’s own read-only server: a tool it also offers is a read tool. Read calls go through that server, so Linear enforces the limit too. |
| **Notion** | None: select **Sign in with Notion** and choose, in Notion, what to share. See [Signing in with OAuth](#signing-in-with-oauth). | By tool name, ignoring the `notion-` every tool starts with: `search`, `fetch`, `get-`, `list-`, `query-` and `view-` tools are read tools, and `create-`, `update-`, `delete-`, `move-` and `duplicate-` tools, among others, are write tools. Anything else waits for you. Agents see the tools without the extra prefix, as `notion_search` and so on. |
| **PostHog** | A personal API key created with the *MCP Server* preset. Choose US or EU Cloud, and optionally limit which features’ tools are offered. | By PostHog’s read-only mode, the same way as Linear. |
| **Rootly** | A Rootly API key (*Account → Manage API keys*). A global key sees every team; a team or personal key reaches less. Optionally name the toolsets to offer. | By Rootly’s read-only mode, the same way as Linear. |
| **Sentry** | A user auth token with `org:read`, `project:read` and `event:read`, plus `project:write` and `event:write` if agents may change things. Optionally name one organization. sentry.io only. | By tool name: `get_`, `find_`, `search_` and `list_` tools are read tools (unless Sentry marks them as making changes), and `create_`, `update_`, `add_`, `delete_` and `remove_` tools are write tools. Anything else waits for you. |
| **Slack** | A Slack app you create for your workspace at *api.slack.com/apps*: turn on the Slack MCP server under *Agents*, and add Hyperbeam’s redirect URL and the user scopes agents should have under *OAuth & Permissions*. Enter its client ID and secret, then sign in. Slack admits only internal or Marketplace apps. | By tool name, ignoring the `slack_` every tool starts with: `search_`, `read_`, `list_` and `get_` tools are read tools, and `send_`, `schedule_`, `create_`, `update_`, `add_` and upload tools are write tools. Slack does not publish its tool names, so expect some to wait for you. |
| **Exa web search** | An Exa API key (*dashboard.exa.ai → API keys*). Searches are billed to it, and what agents search for is sent to Exa. | By tool name: the search tool is a read tool. Exa’s own page fetcher waits for you; see [Web search](#web-search). |

Agents act as the credential’s user, or as the account that signed in. Hyperbeam re-lists each connection’s tools every few hours, and when you select **Re-list tools**.

### Web search

Connect **Exa web search** to give agents a search tool, `exa_web_search_exa`, for people who hold `exa:read`. URLs a search returns are ones the agent may then read with `web_fetch`. See [Web access](/security#web-access).

Exa also offers a tool that fetches any URL. It is hidden until an admin clears it, like any unsorted tool. Leave it hidden unless you accept that agents can then send a URL of their own writing to any site: `web_fetch` exists to avoid exactly that.

### Custom connections

Select **Add custom connection** for any other server that speaks MCP over streamable HTTP, such as an internal tool. Give it:

- a **name**: lowercase letters, digits and dashes. It prefixes the tool names and the permissions, and cannot be changed;
- the server’s **URL**;
- how to send the **credential**: a bearer token, a header you name (such as `X-Api-Key`), an [OAuth sign-in](#signing-in-with-oauth), or none.

**Nothing a custom server offers is usable until you say what it is for.** New tools wait under **Awaiting review**. If the server later changes a tool’s description, input or annotations, that tool is hidden again until you review it, so a server cannot quietly turn a tool you approved into something else. Agents see a tool as it was when you approved it.

Servers that can only run as a local process (stdio) aren’t supported yet.

### Signing in with OAuth

Datadog, Figma, Granola, HubSpot, Notion and Slack, and any custom connection whose credential is **OAuth sign-in**, are connected by signing in rather than pasting a key. Hyperbeam sends you to the service, you approve access there, and you come back with the connection made. Nothing is saved if you abandon the sign-in, and it must be finished within ten minutes by the admin who started it.

**Every agent acts as the account you sign in with**, and reaches whatever that account can. On Slack, that includes sending messages in that person’s name. For anything beyond a small team, sign in with a dedicated account that has been given only what agents should see.

Hyperbeam finds the service’s sign-in from the server’s URL, as the MCP specification describes, and always uses PKCE. Most MCP servers, Notion’s included, let Hyperbeam register itself as a client, so there is nothing else to set up. HubSpot and Slack do not: the connect dialog says where to create a client with them, and asks for its ID and secret. For a custom server that does not, create an OAuth client with the service, give it the redirect URL shown in the dialog (`<HYPERBEAM_PUBLIC_URL>/api/v1/oauth/callback`), and enter its client ID, and its secret if it has one. You can also name the scopes to ask for; otherwise Hyperbeam asks for what the server suggests.

The access and refresh tokens are stored encrypted in the control plane, like every other credential. Hyperbeam renews the access token shortly before it expires, and once more if the service turns it down. If the sign-in can no longer be renewed, because access was revoked or the refresh token ran out, the connection shows **Needs attention**: select **Change**, then **Sign in again**.

### Disconnecting

**Disconnect** deletes the credential, stops agents using the connection straight away, and removes everyone’s permissions for it. A later connection with the same name starts with nobody holding its permissions.

> [!NOTE]
> GitHub App mode, GitLab and the first-class connections are written against the vendors’ documentation but have not yet been tested against the real services. See [What is verified](/status).

An existing Linear integration from an earlier version is moved to the Linear connection automatically, with its key. An earlier version’s Notion integration is not: Notion’s MCP server does not accept the internal integration secret it used, so that secret is deleted on upgrade and an admin signs in to Notion again. People keep the Notion permissions they held.
