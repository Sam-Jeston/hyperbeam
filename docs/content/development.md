---
title: Development
description: Build Noemune from source, run the tests, and work on the web app, landing page and docs.
---

## Repository layout

| Path | Contents |
|---|---|
| `crates/core` | Permissions, runner protocol, workflow format. |
| `crates/server` | API, session manager, tool gateway, git proxy, egress proxy, sandbox drivers. |
| `crates/runner` | The in-sandbox runner, ACP client and mock harness. |
| `crates/cli` | The `noemune` command. |
| `web` | The product UI (React, Radix, Tailwind), embedded in the server binary. |
| `site` | The landing page. |
| `docs` | These docs. |
| `brand` | Shared brand tokens, fonts and build helpers for `web`, `site` and `docs`. |
| `deploy` | Dockerfiles and the Helm chart. |
| `examples` | Example workflows. |

## Build and test

```bash
cargo build -p noemune-runner
NOEMUNE_TEST_DATABASE_URL=postgres://noemune:noemune@127.0.0.1:5440/postgres cargo test --workspace
(cd web && bun run typecheck && bun run build)
```

The end-to-end tests need Postgres. Without `NOEMUNE_TEST_DATABASE_URL` they are skipped.

## The web app

```bash
cd web
bun install
bun run watch     # rebuild dist/ on change
```

In a debug build the server reads `web/dist` from disk, so reload the browser after a rebuild. Release builds embed it.

## Landing page and docs

Both are rendered to static HTML at build time and need no server-side code.

```bash
cd docs && bun install && bun run dev    # http://localhost:4322/docs
cd site && bun install && bun run dev    # http://localhost:4321, with the docs under /docs once built
```

`bun run build` in each writes `dist/`. Docs pages are Markdown files in `docs/content/`, listed in order in `docs/src/nav.ts`. The build fails on a link to a page or heading that doesn’t exist.

They link to each other by path: the docs expect to be served at `/docs` beside the landing page. Override this at build time with `NOEMUNE_DOCS_BASE`, `NOEMUNE_SITE_URL` and `NOEMUNE_DOCS_URL`. Set `NOEMUNE_REPO_URL` to show links to the source and “Edit this page”.

## Brand

`brand/README.md` describes the brand and how to apply it. All colors, fonts and radii come from `brand/tokens.css`; don’t define colors anywhere else.
