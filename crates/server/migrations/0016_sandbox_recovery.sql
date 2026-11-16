-- Recovering from a lost sandbox.
--
-- A sandbox can die under a session through no fault of the agent: killed for
-- memory, evicted, never started. The session is then given a fresh one
-- rather than failed, a bounded number of times in a row.
ALTER TABLE sessions
    -- Sandboxes lost in a row. Back to zero when a turn ends, or a person
    -- sends the session on its way again.
    ADD COLUMN recoveries   integer NOT NULL DEFAULT 0,
    -- A pending session is not given a sandbox before this, so that a
    -- machine under pressure gets a moment to settle between attempts.
    ADD COLUMN launch_after timestamptz;
