-- Device Authorization Grant (RFC 8628): pending browser logins for the TUI.
-- `device_code` is a secret stored only as its SHA-256 hash; `user_code` is a
-- short human-typed pairing code. `user_id` is NULL until approved.

CREATE TABLE IF NOT EXISTS device_grants (
    device_code_hash TEXT PRIMARY KEY,
    user_code        TEXT NOT NULL UNIQUE,
    user_id          UUID REFERENCES users(id) ON DELETE CASCADE,
    created_at       TIMESTAMPTZ NOT NULL,
    expires_at       TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS device_grants_user_code_idx ON device_grants (user_code);
