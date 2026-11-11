-- Model selection, per harness and per session.

ALTER TABLE harnesses
    -- [{id, label}] models people may pick. Empty = whatever the harness offers.
    ADD COLUMN models jsonb NOT NULL DEFAULT '[]',
    -- [{id, label, description}] as last reported by the harness itself.
    ADD COLUMN discovered_models jsonb NOT NULL DEFAULT '[]',
    -- Used when a session does not choose. NULL = the harness's own default.
    ADD COLUMN default_model text,
    -- Environment variable that also receives the chosen model, for harnesses
    -- that read it from there.
    ADD COLUMN model_env text;

-- Requested model. NULL = the harness profile's default.
ALTER TABLE sessions ADD COLUMN model text;

UPDATE harnesses SET model_env = 'ANTHROPIC_MODEL' WHERE builtin AND name = 'claude-code';
UPDATE harnesses SET model_env = 'GEMINI_MODEL' WHERE builtin AND name = 'gemini';
