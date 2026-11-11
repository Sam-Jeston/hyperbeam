-- Hyperbeam schema. One installation is one team.

CREATE TABLE settings (
    key         text PRIMARY KEY,
    value       jsonb NOT NULL,
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE users (
    id             uuid PRIMARY KEY,
    email          text NOT NULL UNIQUE,
    name           text NOT NULL,
    password_hash  text NOT NULL,
    role           text NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
    -- What agents launched by this person may do outside the platform.
    grants         jsonb NOT NULL DEFAULT '[]',
    -- null = every registered repository; otherwise a list of owner/name.
    repo_scope     jsonb,
    disabled       boolean NOT NULL DEFAULT false,
    created_at     timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE invites (
    id           uuid PRIMARY KEY,
    email        text NOT NULL,
    role         text NOT NULL CHECK (role IN ('admin', 'member')),
    grants       jsonb NOT NULL DEFAULT '[]',
    repo_scope   jsonb,
    token_hash   text NOT NULL UNIQUE,
    created_by   uuid NOT NULL REFERENCES users(id),
    created_at   timestamptz NOT NULL DEFAULT now(),
    expires_at   timestamptz NOT NULL,
    accepted_at  timestamptz
);

CREATE TABLE auth_tokens (
    id            uuid PRIMARY KEY,
    user_id       uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash    text NOT NULL UNIQUE,
    kind          text NOT NULL CHECK (kind IN ('session', 'api')),
    name          text NOT NULL DEFAULT '',
    created_at    timestamptz NOT NULL DEFAULT now(),
    last_used_at  timestamptz,
    expires_at    timestamptz
);

-- Encrypted at rest with the installation master key (AES-256-GCM).
CREATE TABLE secrets (
    key         text PRIMARY KEY,
    nonce       bytea NOT NULL,
    ciphertext  bytea NOT NULL,
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE integrations (
    kind        text PRIMARY KEY CHECK (kind IN ('github', 'linear', 'notion')),
    config      jsonb NOT NULL DEFAULT '{}',
    updated_by  uuid REFERENCES users(id),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE repos (
    id              uuid PRIMARY KEY,
    full_name       text NOT NULL UNIQUE,
    default_branch  text NOT NULL DEFAULT 'main',
    description     text NOT NULL DEFAULT '',
    added_by        uuid REFERENCES users(id),
    created_at      timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE harnesses (
    id            uuid PRIMARY KEY,
    name          text NOT NULL UNIQUE,
    title         text NOT NULL,
    description   text NOT NULL DEFAULT '',
    image         text NOT NULL,
    command       jsonb NOT NULL,
    env           jsonb NOT NULL DEFAULT '{}',
    -- [{env, label, kind: api_key|oauth}] credentials this harness accepts.
    credentials   jsonb NOT NULL DEFAULT '[]',
    egress_hosts  jsonb NOT NULL DEFAULT '[]',
    builtin       boolean NOT NULL DEFAULT false,
    created_at    timestamptz NOT NULL DEFAULT now()
);

-- Which credential a harness runs with: the team's, or a person's own.
CREATE TABLE harness_credentials (
    harness_id  uuid NOT NULL REFERENCES harnesses(id) ON DELETE CASCADE,
    -- nil uuid = team-wide credential.
    user_id     uuid NOT NULL,
    env         text NOT NULL,
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (harness_id, user_id)
);

CREATE TABLE skills (
    id           uuid PRIMARY KEY,
    name         text NOT NULL UNIQUE,
    description  text NOT NULL DEFAULT '',
    content      text NOT NULL,
    updated_by   uuid REFERENCES users(id),
    updated_at   timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE workflows (
    id           uuid PRIMARY KEY,
    name         text NOT NULL UNIQUE,
    description  text NOT NULL DEFAULT '',
    yaml         text NOT NULL,
    updated_by   uuid REFERENCES users(id),
    updated_at   timestamptz NOT NULL DEFAULT now()
);

-- A workstream is a piece of work the team can see: a feature, a fix, an
-- investigation. It groups agent sessions and discussion.
CREATE TABLE workstreams (
    id           uuid PRIMARY KEY,
    title        text NOT NULL,
    description  text NOT NULL DEFAULT '',
    status       text NOT NULL DEFAULT 'scoping'
                 CHECK (status IN ('scoping', 'building', 'review', 'done', 'archived')),
    repos        jsonb NOT NULL DEFAULT '[]',
    -- [{kind, title, url}] pull requests, tickets, documents.
    links        jsonb NOT NULL DEFAULT '[]',
    created_by   uuid NOT NULL REFERENCES users(id),
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE comments (
    id             uuid PRIMARY KEY,
    workstream_id  uuid NOT NULL REFERENCES workstreams(id) ON DELETE CASCADE,
    user_id        uuid NOT NULL REFERENCES users(id),
    body           text NOT NULL,
    created_at     timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE sessions (
    id                uuid PRIMARY KEY,
    workstream_id     uuid REFERENCES workstreams(id) ON DELETE SET NULL,
    parent_id         uuid REFERENCES sessions(id) ON DELETE SET NULL,
    title             text NOT NULL,
    role              text NOT NULL DEFAULT 'worker' CHECK (role IN ('worker', 'conductor')),
    status            text NOT NULL DEFAULT 'pending',
    harness_id        uuid NOT NULL REFERENCES harnesses(id),
    created_by        uuid NOT NULL REFERENCES users(id),
    workflow          text,
    repos             jsonb NOT NULL DEFAULT '[]',
    skills            jsonb NOT NULL DEFAULT '[]',
    -- Effective grants, fixed at creation: user ∩ requested.
    grants            jsonb NOT NULL DEFAULT '[]',
    work_branch       text NOT NULL,
    links             jsonb NOT NULL DEFAULT '[]',
    headline          text NOT NULL DEFAULT '',
    summary           text NOT NULL DEFAULT '',
    error             text,
    run_token_hash    text UNIQUE,
    event_seq         bigint NOT NULL DEFAULT 0,
    launches          integer NOT NULL DEFAULT 0,
    created_at        timestamptz NOT NULL DEFAULT now(),
    started_at        timestamptz,
    ended_at          timestamptz,
    last_activity_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX sessions_workstream ON sessions (workstream_id);
CREATE INDEX sessions_parent ON sessions (parent_id);
CREATE INDEX sessions_status ON sessions (status);
CREATE INDEX sessions_created ON sessions (created_at DESC);

-- The append-only log of everything an agent did. This is what teammates watch.
CREATE TABLE session_events (
    session_id  uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    seq         bigint NOT NULL,
    ts          timestamptz NOT NULL DEFAULT now(),
    kind        text NOT NULL,
    data        jsonb NOT NULL,
    PRIMARY KEY (session_id, seq)
);

-- Prompts waiting to be, or already, delivered to a session.
CREATE TABLE session_inputs (
    id            uuid PRIMARY KEY,
    session_id    uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    -- null = sent by the platform (e.g. a child agent finishing).
    user_id       uuid REFERENCES users(id),
    text          text NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    delivered_at  timestamptz,
    completed_at  timestamptz
);
CREATE INDEX session_inputs_pending ON session_inputs (session_id, created_at) WHERE delivered_at IS NULL;

-- Shared knowledge agents read and write: the team's persistent world view.
CREATE TABLE memory (
    key                 text PRIMARY KEY,
    content             text NOT NULL,
    updated_by_user     uuid REFERENCES users(id),
    updated_by_session  uuid REFERENCES sessions(id) ON DELETE SET NULL,
    updated_at          timestamptz NOT NULL DEFAULT now()
);

-- Team-visible feed.
CREATE TABLE activity (
    id             bigserial PRIMARY KEY,
    ts             timestamptz NOT NULL DEFAULT now(),
    user_id        uuid REFERENCES users(id),
    session_id     uuid REFERENCES sessions(id) ON DELETE CASCADE,
    workstream_id  uuid REFERENCES workstreams(id) ON DELETE CASCADE,
    kind           text NOT NULL,
    summary        text NOT NULL,
    data           jsonb NOT NULL DEFAULT '{}'
);
CREATE INDEX activity_ts ON activity (ts DESC);

-- Every attempt by an agent to act outside its sandbox, allowed or not.
CREATE TABLE audit (
    id          bigserial PRIMARY KEY,
    ts          timestamptz NOT NULL DEFAULT now(),
    session_id  uuid REFERENCES sessions(id) ON DELETE SET NULL,
    user_id     uuid REFERENCES users(id),
    action      text NOT NULL,
    target      text NOT NULL DEFAULT '',
    allowed     boolean NOT NULL,
    detail      text NOT NULL DEFAULT ''
);
CREATE INDEX audit_ts ON audit (ts DESC);
CREATE INDEX audit_session ON audit (session_id);
