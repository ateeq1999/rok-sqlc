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

**Status:** ready-for-agent

- [ ] Tests, in both templates:
  - a png/jpeg/webp → 256×256 JPEG stored, `avatar_url` set, GET serves it with cache headers;
  - replacing deletes the old object;
  - DELETE clears it;
  - garbage → 422;
  - > 2 MiB → 413;
  - a decompression bomb (huge dimensions) → 422 quickly;
  - EXIF is gone;
  - a bad file name → 404.
- [ ] Gates pass; the contract has no `avatar_url` gaps.
