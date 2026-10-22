-- Two-factor sign-in (TOTP). `users.totp_secret` (from `0001`) holds the
-- authenticator secret encrypted with AES-256-GCM under
-- `TOTP_ENCRYPTION_KEY` (base64 of the nonce, then the ciphertext); it is
-- set while a setup waits for its first code, and `totp_enabled` turns on
-- once `POST /auth/2fa/enable` confirms one.

-- The time step of the last code accepted, so a code is never accepted
-- twice.
ALTER TABLE users ADD COLUMN totp_last_step BIGINT;

-- Single-use recovery codes, issued when two-factor is enabled. Only the
-- SHA-256 of each is kept; `used_at` spends it.
CREATE TABLE totp_recovery_codes (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    code_hash TEXT NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX totp_recovery_codes_user ON totp_recovery_codes (user_id);

-- A right password on a two-factor account issues a short-lived
-- `two_factor_pending` token, which `POST /auth/2fa/verify` spends.
ALTER TABLE auth_tokens DROP CONSTRAINT auth_tokens_purpose_check;
ALTER TABLE auth_tokens ADD CONSTRAINT auth_tokens_purpose_check CHECK (
    purpose IN ('email_verification', 'password_reset', 'email_change',
                'invitation', 'two_factor_pending')
);
