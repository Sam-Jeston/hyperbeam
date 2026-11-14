---
title: Environments
description: Give agents your project's toolchains by running their sandboxes in an image of your own.
---

An **environment** is a container image with the tools a project needs: compilers, package managers, system libraries, test dependencies. An agent that runs in one starts with all of that installed. Without one, it runs in the harness image, which has git and Node.js and little else.

Environments are separate from [harnesses](/harnesses). When a sandbox starts in an environment, Hyperbeam carries the coding agent into it from the harness image. One environment therefore serves every harness, and one harness every environment.

Environments apply on Kubernetes. The local process driver used in [development](/development) runs agents on your own machine with whatever is installed there, and ignores the environment. It still runs [setup commands](#setup-commands).

## Add an environment

Under **Settings → Environments**, select **Add an environment**.

| Field | Meaning |
|---|---|
| Name | Lowercase letters, digits and dashes. Used in workflows and `--environment`. `none` is reserved. |
| Container image | Any image your cluster can pull, such as `rust:1.90` or the image your CI builds in. |
| Description | Shown where people choose an environment. |

The image must:

- **Be built on glibc.** Debian, Ubuntu and Fedora images are. Alpine images are not, and the agent will not start in one.
- **Have `git` and `sh`.** Add `ca-certificates` too if the image lacks them.
- **Work for user 1000.** Sandboxes do not run as root and cannot become root, so nothing can be installed with `apt-get` once the sandbox is running. Install tools where every user can run them: `/usr/local`, `/opt`, or the system package manager at build time.

The image does not need Node.js, the agent, or anything from Hyperbeam.

If your cluster needs a credential to pull the image, give it one the usual way for your cluster. Hyperbeam does not manage pull secrets.

## Choose where agents run

Each repository can name its environment. Under **Settings → Repositories**, edit the repository and choose one. Agents working on that repository then run in it.

Whoever starts work can choose differently for one session:

- In **New work**, with the **Environment** field. It appears once the team has an environment.
- With `hyperbeam run --environment <name>`.
- With `environment:` in a [workflow](/workflows).

The name `none` runs the session in the harness image, whatever the repository says.

When nobody chooses, the session uses the environment of the first of its repositories that has one. A session with no repositories runs in the harness image.

A [conductor’s](/conductor) workers follow the same rule from their own repositories. If someone chose the conductor’s environment by hand, its workers run there too.

A session keeps its environment for its whole life, including when it resumes in a fresh sandbox. Changing an environment’s image affects sandboxes started afterwards.

## Setup commands

A repository can also have a **setup command**, set in the same dialog. The runner runs it with `sh -c` in each fresh checkout, after every repository is cloned and before the agent starts. Use it for what belongs to the checkout, not the image:

```bash
cargo fetch
```

```bash
bun install --frozen-lockfile && bun run codegen
```

- It runs every time the session gets a sandbox, including on resume.
- It can reach the hosts on the [egress allowlist](/security#network-egress), which covers the common package registries.
- It does not receive the harness credential.
- Its outcome is written to the session log. If it fails, the log shows the end of its output and the agent starts anyway.
- It is stopped after 20 minutes.

Keep slow, unchanging work in the image. A setup command that compiles the world runs again for every sandbox.

## The home directory

A sandbox’s home directory, `/home/agent`, is a fresh writable volume, limited to 2 GiB. Whatever the image keeps in `/home/agent` is copied into it when the sandbox starts, so dotfiles and tools installed there are present.

Two things follow:

- **Large toolchains belong outside the home directory.** `rustup`, for one, installs into the home directory unless told otherwise. If the image holds more than 1 GiB there, nothing is copied and the session log says so. Set `RUSTUP_HOME` and `CARGO_HOME` to paths under `/usr/local`, as the official `rust` image does.
- **Only `/home/agent` is carried over.** `HOME` is always `/home/agent` in a sandbox. Files under `/root` or another user’s home directory are not copied, and user 1000 usually cannot read them.

This applies to harness images too.

## How the agent gets in

A harness image keeps its agent under `/opt/harness`: launchers in `bin`, and a Node.js of its own in `libexec`. When a session runs in an environment, an init container copies that directory out of the harness image, and the sandbox mounts it read-only at the same path.

The agent’s directories go last on the `PATH`. If your image has its own `node`, that is the one your project’s commands use.

Harness images built from `deploy/docker/Dockerfile.harness` have this layout. Images built before environments existed do not: rebuild them before you assign an environment, or the session fails with a message saying the harness image has no agent to carry over. To make an image of your own work with environments, see [Add a harness](/harnesses#add-a-harness).

Copying the agent adds a second or two to each start.

## Limits

- **No image builds.** Hyperbeam runs images; it does not build them from a Dockerfile or `devcontainer.json` in the repository. Build the image where you build your others.
- **Nothing is cached between sandboxes.** Each one starts from the image and a fresh checkout. Dependencies the setup command downloads are downloaded again next time.
