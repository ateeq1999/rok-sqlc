-- Profile photos. `avatar_key` (0001) is the photo's storage key,
-- `avatars/<random>.jpg`, set by `PUT /users/me/avatar`; `avatar_url` is
-- the public URL every user response answers, `/api/v1/avatars/<random>.jpg`
-- (`GET /avatars/{file}`), or `NULL` without a photo. Computed here, so
-- the generated `users` feature answers it too; `rok-sqlc.toml` hides the
-- key.
ALTER TABLE users ADD COLUMN avatar_url TEXT
    GENERATED ALWAYS AS ('/api/v1/' || avatar_key) STORED;
