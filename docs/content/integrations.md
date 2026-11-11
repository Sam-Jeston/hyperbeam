---
title: Integrations
description: Connect GitHub, Linear and Notion, and register the repositories agents may reach.
---

Integrations are set under **Settings → Integrations** by an admin. Their credentials stay in Hyperbeam’s control plane. Agents never see them: every request an agent makes goes through Hyperbeam, which checks it against the permissions of the person who started the agent. Select **Test** on a connected integration to check its credential.

## GitHub

Choose how Hyperbeam authenticates:

- **GitHub App** (recommended for organisations). Create a GitHub App with repository permissions *Contents*, *Pull requests* and *Issues* set to read & write, and install it on the repositories you will register. Paste its App ID and a generated private key. Hyperbeam mints a short-lived token per repository for each request.
- **Access token** (simplest; good for one person or a small team). A fine-grained personal access token limited to the repositories you will register, with *Contents*, *Pull requests* and *Issues* set to read & write. Everything agents do on GitHub appears as this token’s user.

## Repositories

Agents can reach only repositories registered under **Settings → Repositories**, and only the ones their owner [has access to](/permissions#repository-access). Register one as `owner/name`.

Removing a repository stops new agents from using it. Agents already running keep the repositories they started with until they stop. The repository itself is untouched.

## Linear

Paste a Linear API key (Linear: *Settings → Security & access → Personal API keys*) or an OAuth access token. Agents act as that key’s user.

## Notion

Paste the secret of a Notion internal integration. In Notion, share each page agents should reach with that integration.

> [!NOTE]
> Linear, Notion and GitHub App mode are written against the documented APIs but have not yet been tested against the real services. See [What is verified](/status).
