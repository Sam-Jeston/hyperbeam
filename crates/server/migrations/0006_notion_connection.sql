-- Notion is now an MCP connection signed in to with OAuth, like Linear or
-- PostHog, rather than a built-in integration holding an internal
-- integration secret. Notion's hosted MCP server does not accept those
-- secrets, so the old credential cannot be carried over: an admin connects
-- Notion again under Settings → Integrations.
--
-- Where Notion had been connected, people keep their Notion permissions for
-- when it is reconnected. Where it never was, the permissions handed out at
-- setup are dropped, so that Notion, like every connection, starts with
-- nobody holding its permissions.
UPDATE users
SET grants = (SELECT coalesce(jsonb_agg(g), '[]'::jsonb) FROM jsonb_array_elements(grants) AS g WHERE g #>> '{}' NOT LIKE 'notion:%')
WHERE NOT EXISTS (SELECT 1 FROM integrations WHERE kind = 'notion')
  AND grants @? '$[*] ? (@ starts with "notion:")';

UPDATE invites
SET grants = (SELECT coalesce(jsonb_agg(g), '[]'::jsonb) FROM jsonb_array_elements(grants) AS g WHERE g #>> '{}' NOT LIKE 'notion:%')
WHERE NOT EXISTS (SELECT 1 FROM integrations WHERE kind = 'notion')
  AND grants @? '$[*] ? (@ starts with "notion:")';

DELETE FROM secrets WHERE key = 'integration/notion';
DELETE FROM integrations WHERE kind = 'notion';
