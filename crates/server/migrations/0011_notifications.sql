-- Notifications: what a person is told when an agent of theirs needs them
-- (it asked a question, finished a turn, or failed). Each is kept here for
-- the hub in the web UI, and may also be sent as a direct message by a chat
-- app the person has turned on.

CREATE TABLE notifications (
    id          uuid PRIMARY KEY,
    user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    session_id  uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    kind        text NOT NULL CHECK (kind IN ('question', 'finished', 'failed')),
    -- The session's title when this happened.
    title       text NOT NULL,
    -- The question, the agent's closing message, or the error.
    body        text NOT NULL DEFAULT '',
    created_at  timestamptz NOT NULL DEFAULT now(),
    read_at     timestamptz
);
CREATE INDEX notifications_user ON notifications (user_id, created_at DESC);
CREATE INDEX notifications_unread ON notifications (user_id) WHERE read_at IS NULL;

-- The chat apps a person has asked to be notified through. A row means on.
-- The message goes to the chat accounts they have linked (`chat_identities`).
CREATE TABLE notification_channels (
    user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    platform    text NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, platform)
);
