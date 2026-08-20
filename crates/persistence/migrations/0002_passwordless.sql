-- Passwordless: TOTP (authenticator app) is the sole credential. Drop the
-- password column and generalize the single-use challenge table.

-- Accounts that never completed TOTP setup have no secret to migrate to
-- passwordless sign-in; remove them (sessions/challenges cascade).
DELETE FROM users WHERE totp_secret_enc IS NULL;

ALTER TABLE users DROP COLUMN password_hash;
ALTER TABLE users ALTER COLUMN totp_secret_enc SET NOT NULL;

ALTER TABLE login_challenges RENAME TO challenges;
