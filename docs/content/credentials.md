---
title: Credentials
description: Team and personal keys for harnesses, how they are stored, and what to know about subscription tokens.
---

A harness needs a credential, such as a model provider API key, to run. You can set one for the whole team, and each person can set their own.

## Team and personal credentials

Under **Settings → Harnesses**, each harness shows two lines:

- **Team credential**: used by everyone’s agents that don’t have their own. Only admins can set it.
- **Mine**: used by your agents only, instead of the team’s.

When a session starts, Hyperbeam uses its owner’s personal credential if there is one, and the team credential otherwise.

## Subscription tokens

Some harness profiles also accept a personal subscription token (for Claude Code, from `claude setup-token`) as well as an API key. Hyperbeam treats these as personal: only their owner can set one, for their own sessions, never as a team credential.

> [!WARNING]
> Whether a subscription may be used this way is decided by your provider’s terms, which changed more than once during 2026. For a team or anything automated, use API keys.

## How credentials are stored

Credentials are encrypted at rest with AES-256-GCM, using the installation’s master key. Each one is bound to its owner and harness, so a stored credential can’t be moved to another person or harness. They are never shown again after you save them.

Integration and connection credentials for GitHub, Notion, Linear and other services stay in the control plane and never enter a sandbox. See [Security model](/security).

## What the agent can see

A harness credential is delivered to the sandbox of a session that needs it, as the environment variable the harness profile names. The agent process can therefore read it. The [egress allowlist](/security#network-egress) limits where it can send it, but treat harness keys as visible to the agents that use them, and prefer keys scoped to their purpose.

## The master key

The master key is 32 random bytes that encrypt every stored credential. Hyperbeam can get it from one of three places. Set exactly one.

| Source | Setting | Where the key lives |
|---|---|---|
| Given directly | `HYPERBEAM_MASTER_KEY` | In the server’s environment. The Helm chart keeps it in a Kubernetes Secret. |
| AWS Secrets Manager | `HYPERBEAM_MASTER_KEY_AWS_SECRET` | In a secret you create. The server reads it at start. |
| AWS KMS | `HYPERBEAM_MASTER_KEY_AWS_KMS_KEY` | In Postgres, encrypted by a KMS key. The server generates it on first start. |

### Given directly

Generate a key with `hyperbeam-server gen-key` or `openssl rand -base64 32`, and set it as `HYPERBEAM_MASTER_KEY`.

### AWS Secrets Manager

Store a generated key in a secret, and give the server the secret’s name or ARN:

```bash
aws secretsmanager create-secret --name hyperbeam/master-key \
  --secret-string "$(openssl rand -base64 32)"

export HYPERBEAM_MASTER_KEY_AWS_SECRET=hyperbeam/master-key
```

The secret’s value can be the base64 key as plain text, a JSON object with the key under `master_key`, or the 32 raw bytes as a binary secret. The server needs `secretsmanager:GetSecretValue` on the secret, and `kms:Decrypt` if the secret uses a customer managed key.

The server reads the secret once, when it starts. To move an installation that was given its key directly, store that same key in the secret.

### AWS KMS

Give the server the ID, alias or ARN of a symmetric KMS key:

```bash
export HYPERBEAM_MASTER_KEY_AWS_KMS_KEY=alias/hyperbeam
```

On first start the server asks KMS for a new data key and stores it in Postgres, encrypted by your KMS key. On each later start it asks KMS to decrypt the stored key. The unencrypted master key exists only in the server’s memory, so there is no key to hand over or back up separately: a database backup and the KMS key are enough to restore.

The server needs `kms:GenerateDataKey` and `kms:Decrypt` on the key. Both calls carry the encryption context `hyperbeam=master-key`, which you can require in the key policy and which appears in CloudTrail.

> [!WARNING]
> Scheduling the KMS key for deletion makes every stored credential unreadable. Rotating its key material in KMS is safe.

An installation that already holds credentials under a directly supplied key can’t switch to KMS: the server refuses to start rather than generate a key that can’t read them. Use Secrets Manager for such an installation, or enter the credentials again.

### AWS credentials and region

The server finds AWS credentials the way the AWS SDKs do: environment variables, EKS IAM roles for service accounts or Pod Identity, an instance profile, or a shared profile. The region comes from the ARN if you gave one, and from `AWS_REGION` otherwise.

With the Helm chart, set `aws.masterKeySecret` or `aws.kmsKeyId` instead of `masterKey`. See [Install on Kubernetes](/install-kubernetes#keep-the-master-key-in-aws).

### Backups and rotation

> [!IMPORTANT]
> Back up a master key you supply yourself, and don’t change it. Hyperbeam can’t yet rotate the master key: credentials stored under one key can’t be read with another, and would have to be entered again.
