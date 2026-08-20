-- Remove the token-balance revenue model. Tokens are no longer tracked on the
-- user record.

ALTER TABLE users DROP COLUMN token_balance;
