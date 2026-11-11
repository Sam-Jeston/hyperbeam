---
title: Configuration
description: Every server setting that matters, with its environment variable and default.
---

The server is configured with environment variables, or the matching command-line flags. `hyperbeam-server serve --help` lists every option.

## Core

| Variable | Default | Meaning |
|---|---|---|
| `HYPERBEAM_DATABASE_URL` | none (required) | Postgres connection URL. |
| `HYPERBEAM_MASTER_KEY` | none (required) | 32 random bytes, base64. Encrypts stored credentials. See [Credentials](/credentials#the-master-key). |
| `HYPERBEAM_LISTEN` | `0.0.0.0:8080` | Address the server listens on. |
| `HYPERBEAM_PUBLIC_URL` | `http://localhost:8080` | Where people reach the UI. Used in invite links. |
| `HYPERBEAM_INTERNAL_URL` | `http://127.0.0.1:8080` | Where sandboxes reach the server. |

## Sandboxes

| Variable | Default | Meaning |
|---|---|---|
| `HYPERBEAM_SANDBOX_DRIVER` | `process` | `process` runs agents as local processes with no isolation, for development. `kubernetes` runs one pod per session. |
| `HYPERBEAM_IDLE_TIMEOUT_SECS` | `900` | Tear down an idle session’s sandbox after this long. The session can still be resumed. |
| `HYPERBEAM_START_TIMEOUT_SECS` | `600` | Give up on a sandbox that has not connected within this long. |
| `HYPERBEAM_MAX_SESSIONS` | `20` | Most sandboxes running at once. Others queue. |
| `HYPERBEAM_RUNNER_BIN` | next to the server | Process driver: path to `hyperbeam-runner`. |
| `HYPERBEAM_WORKSPACE_ROOT` | `hyperbeam-sandboxes` in the system temp directory | Process driver: where workspaces are created. |

## Kubernetes

| Variable | Default | Meaning |
|---|---|---|
| `HYPERBEAM_K8S_NAMESPACE` | `hyperbeam-agents` | Namespace sandboxes run in. |
| `HYPERBEAM_RUNNER_IMAGE` | `ghcr.io/hyperbeam/runner:latest` | Image that carries the runner binary. Point it at the runner image you built. |
| `HYPERBEAM_K8S_IMAGE_PULL_POLICY` | `IfNotPresent` | Pull policy for sandbox images. |
| `HYPERBEAM_K8S_CPU_LIMIT` | `2` | CPU limit per sandbox. |
| `HYPERBEAM_K8S_MEMORY_LIMIT` | `4Gi` | Memory limit per sandbox. |
| `HYPERBEAM_K8S_WORKSPACE_SIZE` | `10Gi` | Workspace size per sandbox. |

## Network egress

| Variable | Default | Meaning |
|---|---|---|
| `HYPERBEAM_EGRESS_LISTEN` | none | Address for the egress proxy to listen on. Setting it enables the proxy. |
| `HYPERBEAM_EGRESS_ADDR` | none | `host:port` sandboxes use to reach the egress proxy. |
| `HYPERBEAM_EGRESS_ALLOW` | package registries | Hosts every sandbox may reach. The default allows npm, PyPI, crates.io and the Go module proxy. |

Admins can add more hosts under **Settings → General**, and each harness profile adds its own. See [Network egress](/security#network-egress).

## Integrations

These point Hyperbeam at the services’ APIs. Change them only for testing or a self-hosted GitHub.

| Variable | Default |
|---|---|
| `HYPERBEAM_GITHUB_API_URL` | `https://api.github.com` |
| `HYPERBEAM_GITHUB_GIT_URL` | `https://github.com` |
| `HYPERBEAM_NOTION_API_URL` | `https://api.notion.com/v1` |
