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

Integration and connection credentials for GitHub, Linear, Notion and other services stay in the control plane and never enter a sandbox. See [Security model](/security).

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

An installation that already holds credentials under a directly supplied key can’t switch to KMS by changing the setting alone: the server refuses to start rather than generate a key that can’t read them. Move it with [`rotate-master-key`](#rotating-the-master-key).

### AWS credentials and region

The server finds AWS credentials the way the AWS SDKs do: environment variables, EKS IAM roles for service accounts or Pod Identity, an instance profile, or a shared profile. The region comes from the ARN if you gave one, and from `AWS_REGION` otherwise.

With the Helm chart, set `aws.masterKeySecret` or `aws.kmsKeyId` instead of `masterKey`. See [Install on Kubernetes](/install-kubernetes#keep-the-master-key-in-aws).

### Backups

> [!IMPORTANT]
> Back up a master key you supply yourself. Credentials stored under one key can’t be read with another, so changing the setting on its own makes them unreadable. To change the key, rotate it.

## Rotating the master key

`hyperbeam-server rotate-master-key` re-encrypts every stored credential under a new key. It reads the current key from the same settings as the server, and takes the new one as an option:

| New key | Option |
|---|---|
| Given directly | `--new-master-key` (or `HYPERBEAM_NEW_MASTER_KEY`) |
| AWS Secrets Manager | `--new-master-key-aws-secret` |
| AWS KMS | `--new-master-key-aws-kms-key` |

The new key doesn’t have to come from the same kind of source as the current one, so this is also how you move an installation from a directly supplied key to KMS, or back.

Rotation happens in one database transaction: afterwards every credential is under the new key, or, if anything went wrong, nothing has changed. If the current key can’t read a stored credential, the command stops and names it. Running the command again after it has succeeded does no harm.

> [!IMPORTANT]
> Restart the server with the new key straight after rotating. A server that is still running holds the old key: it can’t read stored credentials until it restarts, and a credential saved through it in the meantime has to be entered again.

### A key given directly

```bash
NEW=$(hyperbeam-server gen-key)
hyperbeam-server rotate-master-key --new-master-key "$NEW"
```

Then set `HYPERBEAM_MASTER_KEY` to the new key and restart. Back up the new key before you rotate.

On Kubernetes, run the command in the server’s pod, where the current settings are already in place, and then give the chart the new key. The upgrade restarts the server:

```bash
NEW=$(openssl rand -base64 32)
kubectl -n hyperbeam exec deploy/hyperbeam -- hyperbeam-server rotate-master-key --new-master-key "$NEW"
helm upgrade hyperbeam deploy/helm/hyperbeam -n hyperbeam --reset-then-reuse-values --set masterKey="$NEW"
```

If you manage the Secret yourself (`existingSecret`), update its `master-key` and restart the deployment instead.

### AWS Secrets Manager

Put the new key in the same secret, and name that secret as the new source. Secrets Manager keeps the value it replaced as the secret’s previous version, and the command reads the old key from there:

```bash
aws secretsmanager put-secret-value --secret-id hyperbeam/master-key \
  --secret-string "$(openssl rand -base64 32)"

hyperbeam-server rotate-master-key --new-master-key-aws-secret hyperbeam/master-key
```

Then restart the server. Run the two commands together: a server that restarts between them reads the new key before the credentials have been moved to it.

You can also name a different secret, and point `HYPERBEAM_MASTER_KEY_AWS_SECRET` at it afterwards.

### AWS KMS

Name the KMS key to wrap the new master key under. It can be the one already in use:

```bash
hyperbeam-server rotate-master-key --new-master-key-aws-kms-key alias/hyperbeam
```

The command generates a new master key, re-encrypts the credentials, and replaces the wrapped key in Postgres. Then restart the server, with `HYPERBEAM_MASTER_KEY_AWS_KMS_KEY` changed if you named a different KMS key.

This replaces the master key itself. Rotating the KMS key’s material in AWS is separate, needs nothing from Hyperbeam, and doesn’t change the master key.
