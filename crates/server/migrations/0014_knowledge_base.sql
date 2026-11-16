-- The knowledge base: documents written for people to read, such as reports,
-- investigations and designs. Team notes (`memory`) are short standing facts
-- that agents overwrite as they learn; a document is a dated piece of writing
-- that stays tied to the session and workstream it came out of.
--
-- Where documents go is the `knowledge_base` setting: this table, or a place
-- in Notion. With Notion chosen nothing new is written here.
CREATE TABLE documents (
    id             uuid PRIMARY KEY,
    title          text NOT NULL,
    -- One line saying what the document found or decided, for lists and search.
    summary        text NOT NULL DEFAULT '',
    content        text NOT NULL,
    -- The agent session that published it; null when a person wrote it.
    session_id     uuid REFERENCES sessions(id) ON DELETE SET NULL,
    workstream_id  uuid REFERENCES workstreams(id) ON DELETE SET NULL,
    -- The person who wrote it, or whose session did.
    created_by     uuid REFERENCES users(id),
    -- The person who last edited it by hand, if anyone has.
    updated_by     uuid REFERENCES users(id),
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    search         tsvector GENERATED ALWAYS AS (
        setweight(to_tsvector('english', title), 'A')
        || setweight(to_tsvector('english', summary), 'B')
        || setweight(to_tsvector('english', content), 'C')
    ) STORED
);
CREATE INDEX documents_search ON documents USING gin (search);
CREATE INDEX documents_updated ON documents (updated_at DESC);
CREATE INDEX documents_workstream ON documents (workstream_id) WHERE workstream_id IS NOT NULL;
