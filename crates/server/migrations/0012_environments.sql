-- Environments: container images holding a team's toolchains. A sandbox that
-- runs in one gets its coding agent carried in from the harness image, so the
-- same environment serves every harness.
CREATE TABLE environments (
    id           uuid PRIMARY KEY,
    name         text NOT NULL UNIQUE,
    description  text NOT NULL DEFAULT '',
    image        text NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now()
);

-- The environment agents working on a repository run in unless told otherwise,
-- and a command run in each fresh checkout before the agent starts.
ALTER TABLE repos ADD COLUMN environment_id uuid REFERENCES environments(id) ON DELETE SET NULL;
ALTER TABLE repos ADD COLUMN setup_command text NOT NULL DEFAULT '';

-- Settled when the session is created. NULL runs in the harness image.
ALTER TABLE sessions ADD COLUMN environment_id uuid REFERENCES environments(id) ON DELETE SET NULL;
