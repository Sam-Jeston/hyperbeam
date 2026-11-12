-- A question an agent has put to people (or to its conductor) and not yet
-- had answered: {id, text, options, to, asked_at}. It outlives the sandbox,
-- so an agent that fell asleep waiting still shows as needing an answer.
ALTER TABLE sessions ADD COLUMN question jsonb;
