-- Named agents and implement-and-review loops.
--
-- A conductor started from a workflow that declares agents carries them, and
-- the workflow's loop, as `plan`: {agents: {name: {...}}, loop: {implement,
-- review, max_rounds}}. It is a snapshot, so editing the workflow does not
-- change work already under way.
ALTER TABLE sessions
    ADD COLUMN plan            jsonb,
    -- The plan's agent a worker was started as.
    ADD COLUMN agent           text,
    -- The worker's job ends with a verdict rather than only a report.
    ADD COLUMN reports_verdict boolean NOT NULL DEFAULT false;

-- What a reviewing agent concluded. Each verdict from a loop's reviewer ends
-- a round of that loop.
CREATE TABLE verdicts (
    id            uuid PRIMARY KEY,
    session_id    uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    conductor_id  uuid REFERENCES sessions(id) ON DELETE CASCADE,
    agent         text,
    verdict       text NOT NULL CHECK (verdict IN ('approve', 'request_changes')),
    findings      text NOT NULL DEFAULT '',
    -- Its conductor has been told.
    delivered     boolean NOT NULL DEFAULT false,
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX verdicts_conductor ON verdicts (conductor_id, created_at);
CREATE INDEX verdicts_session ON verdicts (session_id, created_at);
