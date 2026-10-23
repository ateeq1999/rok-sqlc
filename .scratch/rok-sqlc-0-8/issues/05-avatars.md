# 05: Avatars (both templates)

**What to build:** decision B7, in both templates, on 04's `Storage`.

- **`PUT /users/me/avatar`** (`account` feature):
  - Takes a raw body of at most 2 MiB (413), decoded with `image` (jpeg, png, webp features only) under `image::Limits` (8192×8192, a memory cap). An undecodable image answers `422`.
  - Centre-crops to 256×256 and re-encodes as JPEG (quality ~85; EXIF stripped by re-encoding), storing it as `avatars/<random>.jpg`.
  - Sets `users.avatar_key` and deletes the previous object (best effort). Answers `200 UserResponse`.
  - Decoding and encoding run in `spawn_blocking`.
- **`DELETE /users/me/avatar`:** clears `avatar_key`, deletes the object, and answers `200 UserResponse` with `avatar_url: null`.
- **`GET /avatars/{file}`** (public): validates `file` against the generated name shape (else `404`), and answers `image/jpeg` with `Cache-Control: public, max-age=31536000, immutable`.
- **`avatar_url`:** every `UserResponse` (auth, account, and the generated `users` feature where `avatar_key` maps; check how the generated response handles it, likely via the scaffolded mapping) answers `/api/v1/avatars/<key>` or null. Remove the contract tests' `avatar_url` `KNOWN` lines.
- The image-processing code is framework-free, in `shared/` (byte-identical). `dto.rs`/`service.rs` stay byte-identical.

**Blocked by:** 04

**Status:** resolved

- [x] Tests, in both templates:
  - a png/jpeg/webp → 256×256 JPEG stored, `avatar_url` set, GET serves it with cache headers;
  - replacing deletes the old object;
  - DELETE clears it;
  - garbage → 422;
  - > 2 MiB → 413;
  - a decompression bomb (huge dimensions) → 422 quickly;
  - EXIF is gone;
  - a bad file name → 404.
- [x] Gates pass; the contract has no `avatar_url` gaps.

## Comments

Done on `rok-sqlc-0-8-05`.

- **`avatar_url` everywhere, without a generator change.** `migrations/0005_avatars.sql` adds `users.avatar_url TEXT GENERATED ALWAYS AS ('/api/v1/' || avatar_key) STORED` (the pattern `has_password` already uses). `avatar_key` stays in `[api.hidden]`; the generated `users` feature's `User`/`UserResponse` pick the column up on `rok-sqlc g` (checked-in output regenerated), and the hand-written `auth`/`account` `UserResponse`s map `row.avatar_url`. Trade-off: the `/api/v1/` prefix lives in the schema too. The 8 `avatar_url` `KNOWN` lines are gone in both contracts; `users.rs`'s hidden-columns test lists `avatar_url`.
- **Image code (`shared/thumbnail.rs`, byte-identical, in the drift test's `SHARED`).** `square_jpeg(&[u8], side) -> Result<Vec<u8>, AppError>`: JPEG/PNG/WebP only (format guessed from the bytes), `image::Limits` (8192×8192, `max_alloc` 128 MiB), `resize_to_fill` (centre crop, Lanczos3), alpha flattened onto white, JPEG quality 85. Undecodable or over the limits → `AppError::Validation` (`422`). Unit tests: three formats, centre kept, transparency, EXIF dropped, garbage, a PNG claiming 100000×100000.
- **`account` feature.** `AccountService { pool, storage }` (`Clone`, `new(pool, storage)`); `set_avatar` (2 MiB → `413`, `spawn_blocking`, `put("avatars/<32 hex>.jpg", jpeg, "image/jpeg")`, then a transaction `LockAvatarKey` (`FOR UPDATE`) + `SetAvatarKey`; the new object is deleted if that fails, the old one after commit, best effort), `remove_avatar`, `avatar(file)` (name shape checked, else `404`). Constants `AVATAR_MAX_BYTES`, `AVATAR_SIDE`, `AVATAR_CACHE_CONTROL` in `dto.rs`. Queries appended to `queries/account.sql`.
- **Routes.** actix: `/api/v1/users/me/avatar` resource with `PayloadConfig::new(2 MiB)`, handler maps the `413` error to `PayloadTooLarge`; axum: `DefaultBodyLimit::max(2 MiB)` on that route, `BytesRejection` mapped the same way. `GET /api/v1/avatars/{file}` is public: `image/jpeg`, `Cache-Control: public, max-age=31536000, immutable`, `nosniff`. axum's `with_storage` also updates `account`. `openapi.rs` lists the three handlers; `COVERED` gained the three operations and the mismatch test two lines.
- **Tests.** `tests/avatars.rs` (9, same in both): the ticket's list plus `401` without a token, and that `/users/me` and the admin's `/users/{id}` answer the URL.
- **Dependencies.** `image = { version = "0.25", default-features = false, features = ["jpeg", "png", "webp"] }` in both manifests and READMEs; `Cargo.lock` +12 (actix) / +14 (axum) packages. READMEs and `AGENTS.md` describe the photo.
- **Deviation.** The contract's description says GIF too; B7 and this ticket say jpeg/png/webp, which is what is accepted.
