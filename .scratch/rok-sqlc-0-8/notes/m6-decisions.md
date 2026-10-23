# Milestone 6 (0.8.0): 2FA, API keys, media and avatars (B1–B7)

Settled with the maintainer on 2026-10-21. This note is the authority for `../spec.md`. Builds on ADR 0016–0019, the M5 decisions (`../../rok-sqlc-0-6/notes/m5-auth-decisions.md`) and the axum decisions (`../../rok-sqlc-0-7/notes/axum-decisions.md`).

- **B1: Scope and release.** 0.8.0, built in both templates at once. `dto.rs`/`service.rs` are byte-identical across them (drift test); handlers and routes are per framework. Tickets: CHECK-constraint changes in the generator, 2FA, API keys, storage port + media, avatars, docs/release.
- **B2: 2FA.**
  - **Codes:** `totp-rs` TOTP (SHA-1, 6 digits, 30 s, ±1 step of drift).
  - **Secret at rest:** AES-256-GCM under `TOTP_ENCRYPTION_KEY` (32 bytes, checked at startup like `JWT_SECRET`).
  - **Replay:** a reused code is refused; the last accepted time step is stored.
  - **Recovery codes:** 10 single-use codes, shown once at enable, stored as SHA-256.
  - **Setup:** answers the secret, the provisioning URI and a QR data URI. 2FA isn't enforced until `enable` confirms a code.
  - **Login:** the right password on a 2FA account answers `{ requires_totp, pending_token }`, with a 5-minute single-use `two_factor_pending` token. `verify` takes the pending token plus a code or recovery code. A wrong code counts toward the lockout (5 failures, 15 min).
  - **Disable:** needs the current password. Enabling and disabling both mail a notice.
- **B3: CHECK-constraint changes.** rok-sqlc's migration parser follows `ALTER TABLE … DROP CONSTRAINT <name>` and `ADD CONSTRAINT <name> CHECK (…)`, so value-list enums (such as `AuthTokenPurpose`) update. The template's `0003` migration uses it to add `two_factor_pending`. This lifts the limitation `docs/guide/auth.md` documents.
- **B4: API keys.**
  - **Format:** `rok_<8-char prefix>_<32 random chars>`. The prefix and a SHA-256 of the key are stored; the raw key is shown once.
  - **Sending:** as `Authorization: Bearer rok_…` (told apart from a JWT by the prefix) or as `X-API-Key`.
  - **Scope:** a `read` key may only call GET/HEAD/OPTIONS; anything else answers `403`.
  - **Lifetime:** `expires_in_days` is optional, 1–365. Expired or revoked keys answer `401`. `last_used_at` is updated at most once a minute.
  - **Owner checks:** the owner's `is_active`/soft delete apply on every request.
  - **Limits:** a key can't reach `/auth/*` or `/api-keys` (`403`). A user has at most 25 active keys.
  - **Revoke** sets `revoked_at`, and the list shows only active keys.
- **B5: Storage port.** `Storage` (`put`/`get`/`delete` by key) in `shared/`, implemented with `object_store`.
  - `STORAGE_URL`: `file:///…` locally (the development default) or `s3://bucket` (S3-compatible, credentials from the environment).
  - Keys are server-generated UUIDs. Tests use the in-memory store.
- **B6: Media.**
  - **Upload:** a raw body, at most `MEDIA_MAX_UPLOAD_BYTES` (default 10 MiB) → `413`. The type is detected from the bytes (`infer`) and checked against `MEDIA_ALLOWED_TYPES` (default: jpeg, png, webp, gif, pdf) → `415`. The optional `X-Filename` header gives the original name.
  - **Writes:** store the bytes, then insert the row, deleting the bytes if the insert fails. Delete removes the row, then the bytes (best effort, logged).
  - **Content:** the detected `Content-Type`, `X-Content-Type-Options: nosniff`, and `Content-Disposition: attachment` for anything that isn't an image.
  - **Access:** owner-scoped (another user's file is a `404`). `url` is `/api/v1/media/{id}/content`. Lists are paginated, newest first.
- **B7: Avatars.**
  - **Upload:** `PUT` takes a raw body of at most 2 MiB, decoded with `image` (jpeg, png, webp), with bomb limits (8192×8192 and a memory cap). An undecodable image answers `422`.
  - **Processing:** a 256×256 centre crop, re-encoded as JPEG, which strips EXIF. Stored as `avatars/<random>.jpg` through `Storage`; the old one is deleted.
  - **Serving:** `GET /avatars/{file}` is public, with `Cache-Control: public, max-age=31536000, immutable`.
  - **`avatar_url`:** `UserResponse.avatar_url` = `/api/v1/avatars/<key>` or null. The contract tests' `avatar_url` `KNOWN` gaps go away.
