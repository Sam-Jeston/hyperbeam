-- MCP connections: external MCP servers the tool gateway proxies to. First-class
-- ones (`preset` set) are named after their preset; custom ones are named by
-- an admin. Credentials live in `secrets` under `connection/<name>`.
CREATE TABLE connections (
    name                text PRIMARY KEY,
    preset              text,
    title               text NOT NULL,
    url                 text NOT NULL,
    -- How the credential is sent: {"header": "Authorization", "prefix": "Bearer "}, or {} for none.
    auth                jsonb NOT NULL DEFAULT '{}',
    -- Extra, non-secret headers sent on every request.
    headers             jsonb NOT NULL DEFAULT '{}',
    -- Preset options (e.g. PostHog region).
    options             jsonb NOT NULL DEFAULT '{}',
    -- What the server last said it offers, with a fingerprint per tool.
    tools               jsonb NOT NULL DEFAULT '[]',
    -- Admin decisions: {"<tool>": {"level": "read|write|hidden", "fingerprint": "..."}}.
    approvals           jsonb NOT NULL DEFAULT '{}',
    tools_error         text,
    tools_refreshed_at  timestamptz,
    created_by          uuid REFERENCES users(id),
    updated_by          uuid REFERENCES users(id),
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    CHECK (preset IS NULL OR preset = name)
);
