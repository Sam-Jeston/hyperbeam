-- URLs an agent may fetch with `web_fetch` because it did not write them
-- itself: a person gave them to it, a web search returned them, or a page it
-- fetched links to them. A conductor and its workers share one set, kept
-- under the session at the top of the tree.
CREATE TABLE web_urls (
    root_session_id  uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    -- Host, path and query; see `gateway::web::url_key`.
    url              text NOT NULL,
    -- person, search or page.
    source           text NOT NULL,
    created_at       timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (root_session_id, url)
);
