---
title: Knowledge base
description: Where the reports, investigations and designs your agents write are published, in Hyperbeam or in Notion.
---

The **knowledge base** holds what your agents find out. When an agent writes a report, the findings of an investigation, a design or any other document meant for people to read, it publishes it to the knowledge base instead of leaving a file in its sandbox, where it would be lost when the sandbox is torn down.

Ask for a document the way you would ask a colleague:

```text
Work out why checkout got slower last week and write up what you find.
```

The agent publishes its write-up and gives the link in its reply. The same link appears on the session under **Produced**, on its workstream under **Links**, and in **Activity**.

## Documents and team notes

Both are things agents write down, for different reasons:

| | Documents | [Team notes](/library#team-notes) |
|---|---|---|
| What it is | The output of a piece of work: a report, an investigation, a design | A standing fact every session should know: a convention, a decision, how to run something |
| Written | Once, when the work is done. Revised by the work that wrote it | Over and over, by any agent that learns something |
| Found by | Searching, and from the session and workstream it came out of | Its key, listed to every agent when it starts |
| Read by | People first, and agents looking for earlier findings | Agents first |

## Where documents are kept

Admins choose under **Knowledge base → Where documents go**.

### In Hyperbeam

The default. Documents are stored in Hyperbeam, and **Knowledge base** in the sidebar is where the team reads them:

- **Search** by what a document says or by its title.
- **Open** one to read it, with the agent session that wrote it, who that session was working for, and its workstream.
- **Edit** or **delete** one. Anyone on the team can, as with skills and team notes. Deleting a document also removes its link from the session and workstream.
- **New document** publishes one you wrote yourself.

Each document has a title, a one-sentence summary shown in lists and search results, and its content in Markdown. Left without a summary, a document is summarised by its first line of prose.

Agents search the knowledge base too, so a later session can build on earlier findings rather than repeat them.

### In Notion

Each document becomes a page in Notion, under a page you choose. Hyperbeam keeps only the setting: the team reads and searches the documents in Notion, and **Knowledge base** in Hyperbeam shows where they go.

1. [Connect Notion](/integrations#signing-in-with-oauth) under **Settings → Integrations**, if it is not connected yet.
2. In Notion, open the page documents should be created under and copy its link (**Share → Copy link**). The account Hyperbeam is signed in to Notion with must be able to add pages to it.
3. In Hyperbeam, open **Knowledge base → Where documents go**, choose **In Notion**, and paste the link.

Each page ends with a line saying which agent session wrote it and for whom, with a link back to the session.

Two things to know about permissions:

- **Publishing needs no Notion permission.** A session publishes under the chosen page whether or not it holds `notion:write`. The setting is what allows it, and it reaches only that page: the agent supplies a title and the content, and Hyperbeam creates the page.
- **Finding earlier documents needs `notion:read`.** With documents in Notion, agents search them with Notion’s own search tool, so only sessions that may read Notion can.

Documents published before you chose Notion stay in Hyperbeam, readable under **Still in Hyperbeam**. Switching back to **In Hyperbeam** keeps the Notion page you chose, in case you switch again; pages already in Notion stay there.

> [!NOTE]
> Publishing to Notion is tested against a stand-in for Notion’s MCP server, not yet against the real service. See [Testing and limits](/status).

## What agents are told

Every agent is told about the knowledge base when it starts: to publish what it writes for people there, to give the link in its reply, and to look for earlier documents before repeating an investigation. Files a task asks for in a repository, such as a `README` or a design note kept beside the code, still go in the repository.

Agents use three tools, which need no grant:

| Tool | What it does |
|---|---|
| `hyperbeam_knowledge_publish` | Publish a document: a title, the content, and a one-sentence summary. With the `id` of a document the same session or workstream published earlier, replace it. |
| `hyperbeam_knowledge_search` | Search documents kept in Hyperbeam, or list the newest. |
| `hyperbeam_knowledge_read` | Read one document kept in Hyperbeam. |

An agent can replace only a document that its own session or workstream published. For anything else it publishes a new document. With documents in Notion, only `hyperbeam_knowledge_publish` is offered, and it always creates a new page.

A document can be up to 200 KB.
