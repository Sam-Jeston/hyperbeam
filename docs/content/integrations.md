---
title: Integrations
description: Connect GitHub, Notion, first-class services like Linear, PostHog and Sentry, and any other MCP server.
---

Integrations are set under **Settings → Integrations** by an admin. Their credentials stay in Hyperbeam’s control plane. Agents never see them: every request an agent makes goes through Hyperbeam, which checks it against the permissions of the person who started the agent. Select **Test** on a connected integration to check its credential.

## GitHub

Choose how Hyperbeam authenticates:

- **GitHub App** (recommended for organisations). Create a GitHub App with repository permissions *Contents*, *Pull requests* and *Issues* set to read & write, and install it on the repositories you will register. Paste its App ID and a generated private key. Hyperbeam mints a short-lived token per repository for each request.
- **Access token** (simplest; good for one person or a small team). A fine-grained personal access token limited to the repositories you will register, with *Contents*, *Pull requests* and *Issues* set to read & write. Everything agents do on GitHub appears as this token’s user.

## Repositories

Agents can reach only repositories registered under **Settings → Repositories**, and only the ones their owner [has access to](/permissions#repository-access). Register one as `owner/name`.

Removing a repository stops new agents from using it. Agents already running keep the repositories they started with until they stop. The repository itself is untouched.

## Notion

Paste the secret of a Notion internal integration. In Notion, share each page agents should reach with that integration.

## Connected services

Everything else is a **connection**: an MCP server that agents reach through Hyperbeam’s tool gateway. Hyperbeam calls the server from the control plane with the connection’s credential, so agents never see the credential or reach the server directly. A connection’s tools appear to agents as `<name>_<tool>`, for example `sentry_search_issues`.

Each of a connection’s tools is a **read** tool, a **write** tool, or **hidden**. Agents use read tools with `<name>:read` and write tools with `<name>:write`, which you grant per person under [People & permissions](/permissions). A new connection’s permissions are granted to nobody, its creator included, until you tick them.

Select **Tools** on a connection to see what it offers and change what each tool is for. Your choice for a tool overrides whatever Hyperbeam decided automatically.

### First-class services

These connect with one credential, and Hyperbeam sorts their tools into read and write for you.

| Service | Credential | How tools are sorted |
|---|---|---|
| **Linear** | A Linear API key (*Settings → Security & access → Personal API keys*). A key with only the Read permission limits every agent to reading. | By Linear’s own read-only server: a tool it also offers is a read tool. Read calls go through that server, so Linear enforces the limit too. |
| **PostHog** | A personal API key created with the *MCP Server* preset. Choose US or EU Cloud, and optionally limit which features’ tools are offered. | By PostHog’s read-only mode, the same way as Linear. |
| **Sentry** | A user auth token with `org:read`, `project:read` and `event:read`, plus `project:write` and `event:write` if agents may change things. Optionally name one organization. sentry.io only. | By tool name: `get_`, `find_`, `search_` and `list_` tools are read tools (unless Sentry marks them as making changes), and `create_`, `update_`, `add_`, `delete_` and `remove_` tools are write tools. Anything else waits for you. |

Agents act as the credential’s user. Hyperbeam re-lists each connection’s tools every few hours, and when you select **Re-list tools**.

### Custom connections

Select **Add custom connection** for any other server that speaks MCP over streamable HTTP, such as an internal tool. Give it:

- a **name**: lowercase letters, digits and dashes. It prefixes the tool names and the permissions, and cannot be changed;
- the server’s **URL**;
- how to send the **credential**: a bearer token, a header you name (such as `X-Api-Key`), or none.

**Nothing a custom server offers is usable until you say what it is for.** New tools wait under **Awaiting review**. If the server later changes a tool’s description, input or annotations, that tool is hidden again until you review it, so a server cannot quietly turn a tool you approved into something else. Agents see a tool as it was when you approved it.

Servers that can only run as a local process (stdio), and servers that sign in only with OAuth, aren’t supported yet.

### Disconnecting

**Disconnect** deletes the credential, stops agents using the connection straight away, and removes everyone’s permissions for it. A later connection with the same name starts with nobody holding its permissions.

> [!NOTE]
> Notion, GitHub App mode and the first-class connections are written against the vendors’ documentation but have not yet been tested against the real services. See [What is verified](/status).

An existing Linear integration from an earlier version is moved to the Linear connection automatically, with its key.
