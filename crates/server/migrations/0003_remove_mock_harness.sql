-- The scripted mock harness is no longer shipped; it lives on only as a test fixture.
-- Remove the builtin profile where no session ever ran on it. Where sessions did, keep
-- it as part of their history, but no longer as a built-in profile.
DELETE FROM harnesses h
WHERE h.builtin AND h.name = 'mock'
  AND NOT EXISTS (SELECT 1 FROM sessions s WHERE s.harness_id = h.id);
UPDATE harnesses SET builtin = false WHERE builtin AND name = 'mock';
