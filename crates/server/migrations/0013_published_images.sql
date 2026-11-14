-- Harness images are published under the repository's own name on GitHub
-- Container Registry. Profiles still naming the old placeholder, which never
-- existed, are pointed at the published images.
UPDATE harnesses
SET image = replace(image, 'ghcr.io/hyperbeam/', 'ghcr.io/sam-jeston/hyperbeam/')
WHERE image LIKE 'ghcr.io/hyperbeam/%';
