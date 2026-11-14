---
title: Set up a team
description: From a fresh installation to a team that can start agents, in five steps.
---

The first person to open a new installation creates the team and becomes its **owner**. Then:

1. **Connect GitHub or GitLab** under **Settings → Integrations**. On GitHub, use a GitHub App for an organisation: Hyperbeam then mints a short-lived token per repository per request. A fine-grained access token works for one person. On GitLab, use a project or group access token. See [Integrations](/integrations).
2. **Register repositories** under **Settings → Repositories**, as `owner/name`. Agents can reach only these.
3. **Give a harness a credential** under **Settings → Harnesses**: a team API key, or each person’s own. See [Credentials](/credentials).
4. **Invite people** from **People & permissions**, and tick what each person’s agents may do. See [People and permissions](/permissions).
5. Optionally connect services such as **Linear**, **Notion**, **PostHog**, **Sentry** or your own MCP servers ([Integrations](/integrations)), choose a **default harness** under **Settings → General**, give agents your toolchains with an [environment](/environments), and add [skills and workflows](/library) in the **Library**.

## Invites

Select **Invite someone**, enter their email, choose a role and what their agents may do, and select **Create invite link**.

Hyperbeam does not send email: copy the link and pass it on yourself. A link works once and expires after seven days. Pending invites are listed on **People & permissions**, where you can revoke them.

## General settings

Under **Settings → General**, admins can change:

- the **team name**;
- the **default harness**, preselected in **New work** and used by workflows that don’t name one;
- **extra hosts every sandbox may reach**, added to the [egress allowlist](/security#network-egress).

The idle timeout and the maximum number of concurrent sandboxes are set when the server starts. See [Configuration](/configuration).
