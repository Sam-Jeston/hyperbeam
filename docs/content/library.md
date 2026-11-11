---
title: Skills and team notes
description: What your team teaches its agents, kept in one place and shared by every session.
---

The **Library** holds what your team teaches its agents. Anyone on the team can add to it.

## Skills

A skill is a set of instructions an agent reads when the work calls for it: how you write migrations, how to run the test suite, your review checklist. Skills use the open `SKILL.md` format and work with every harness.

Each skill has:

- a **name**: lowercase letters, digits and dashes, such as `database-migrations`;
- **when to use it**: one line agents read to decide whether the skill applies, so be specific;
- **instructions**: Markdown.

Choose skills for a session under **Skills and permissions** in **New work**, with `--skill` on the command line, or in a workflow’s `skills` list. They are installed into each sandbox at `.claude/skills/` and `.agents/skills/`.

> [!TIP]
> Write down something you find yourself explaining to agents more than once. That is a skill.

## Team notes

Team notes are durable knowledge shared by every agent on the team: conventions, decisions, how things are run. Agents read these notes and add to them as they learn, so later sessions start out knowing more.

Each note has a **key** (lowercase letters, digits and dashes) and free-form content. Notes written by an agent are marked as such and link to the session that last wrote them. You can seed, edit and delete notes yourself.

## Workflows

Workflows also live in the Library. See [Workflows](/workflows).
