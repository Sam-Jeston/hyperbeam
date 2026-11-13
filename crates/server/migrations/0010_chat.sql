-- Chat apps: Slack and Discord as places to start and direct agents from.
-- A chat thread is bound to a session; everything a person does there goes
-- through the same checks as the web UI, as the Hyperbeam user they are
-- linked to.

-- One bot per platform. Its tokens are in `secrets` under `chat/<platform>/…`.
CREATE TABLE chat_apps (
    platform    text PRIMARY KEY,
    -- {auto_link_email, workspace, bot_name, bot_user_id, team_id}
    config      jsonb NOT NULL DEFAULT '{}',
    updated_by  uuid REFERENCES users(id),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

-- Who a chat account is in Hyperbeam. Nobody without a row here can start or
-- direct an agent from chat.
CREATE TABLE chat_identities (
    platform      text NOT NULL,
    external_id   text NOT NULL,
    user_id       uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    display_name  text NOT NULL DEFAULT '',
    -- `email`: matched on the address the workspace holds. `code`: the person
    -- confirmed a one-time link while signed in.
    linked_by     text NOT NULL,
    linked_at     timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (platform, external_id)
);
CREATE INDEX chat_identities_user ON chat_identities (user_id);

-- One-time links the bot hands out so a person can claim their chat account.
CREATE TABLE chat_link_codes (
    code_hash     text PRIMARY KEY,
    platform      text NOT NULL,
    external_id   text NOT NULL,
    display_name  text NOT NULL DEFAULT '',
    expires_at    timestamptz NOT NULL
);

-- Channels an admin has enabled, with what an agent started there gets.
CREATE TABLE chat_channels (
    platform    text NOT NULL,
    channel_id  text NOT NULL,
    name        text NOT NULL DEFAULT '',
    repos       jsonb NOT NULL DEFAULT '[]',
    -- null = the team's default harness.
    harness     text,
    role        text NOT NULL DEFAULT 'worker' CHECK (role IN ('worker', 'conductor')),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (platform, channel_id)
);

-- A chat thread and the session it talks to.
CREATE TABLE chat_threads (
    id                 uuid PRIMARY KEY,
    platform           text NOT NULL,
    channel_id         text NOT NULL,
    -- Slack: the thread's root timestamp. Discord: empty, a thread is a channel.
    thread_id          text NOT NULL DEFAULT '',
    -- null while the person is still choosing a repository.
    session_id         uuid REFERENCES sessions(id) ON DELETE CASCADE,
    started_by         uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- The start that is waiting on that choice: {prompt, context, harness, role, repos, message_id}.
    pending            jsonb,
    -- The message that shows the session's status, edited in place.
    status_message_id  text,
    -- A digest of what it last said, so it is edited only when that changes.
    status_shown       text,
    -- Session events up to here have been relayed to the thread.
    last_seq           bigint NOT NULL DEFAULT 0,
    created_at         timestamptz NOT NULL DEFAULT now(),
    UNIQUE (platform, channel_id, thread_id)
);
CREATE INDEX chat_threads_session ON chat_threads (session_id);
