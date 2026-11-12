-- Repositories can live on more than one code host. GitLab joins GitHub as a
-- built-in integration, and every repository records which host it is on.
-- Which integrations exist is checked by the server, as it is for
-- connections, so the list no longer needs repeating here.
ALTER TABLE integrations DROP CONSTRAINT IF EXISTS integrations_kind_check;

ALTER TABLE repos ADD COLUMN host text NOT NULL DEFAULT 'github';
