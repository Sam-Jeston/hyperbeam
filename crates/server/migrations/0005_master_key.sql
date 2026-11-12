-- When the master key is kept under a key management service, it is generated
-- on first start and stored here wrapped (encrypted by that service). The
-- plaintext key only ever exists in the server's memory. Empty when the key
-- is supplied directly or read from a secret store.
CREATE TABLE master_key (
    singleton   boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    provider    text NOT NULL CHECK (provider IN ('aws-kms')),
    -- The wrapping key the server was configured with when this was created.
    key_id      text NOT NULL,
    wrapped     bytea NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now()
);
