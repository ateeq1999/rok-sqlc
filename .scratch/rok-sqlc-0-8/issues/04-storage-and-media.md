# 04: The storage port and media (both templates)

**What to build:** decisions B5 and B6, in both templates.

- **`shared/storage/` (byte-identical in both):**
  - A `Storage` port (`#[async_trait]`, `Arc<dyn Storage>`): `put(key, bytes, content_type)`, `get(key) -> Option<(bytes, content_type)>` or a stream (pick, and justify it), `delete(key)`.
  - One `object_store` implementation, chosen by `STORAGE_URL` (`file:///…` default under the project dir, `s3://bucket`, `memory://` for tests).
  - Config in `config.rs`: `STORAGE_URL`, `MEDIA_MAX_UPLOAD_BYTES` (10 MiB), `MEDIA_ALLOWED_TYPES` (jpeg, png, webp, gif, pdf). `AppState` gains `storage`.
- **Feature `media`** (hand-written, standard layout; `media` stays in `[api] exclude`):
  - `POST /media`: a raw `application/octet-stream` body, limited per route by `MEDIA_MAX_UPLOAD_BYTES` (413). The type is detected with `infer` and checked against the allow-list (415). The `X-Filename` header is optional and length-limited. Store under a UUID key, then insert the row (deleting the object if the insert fails). Answers `201 MediaResponse` (`url` = `/api/v1/media/{id}/content`).
  - `GET /media`: paginated, newest first, owner-scoped.
  - `GET /media/{id}`: metadata. Another user's file answers `404`.
  - `DELETE /media/{id}`: deletes the row, then the object (best effort, logged), and answers `204`.
  - `GET /media/{id}/content`: the bytes with the detected `Content-Type`, `X-Content-Type-Options: nosniff`, and `Content-Disposition: attachment` for anything that isn't an image.
  - Queries in `queries/media.sql`.
- `dto.rs`/`service.rs` are byte-identical across the templates (drift test). The contract covers the five paths.

**Blocked by:** None. It runs alongside 01 and 03; keep `AppState`/`config.rs` changes additive.

**Status:** ready-for-agent

- [ ] PostgreSQL tests (memory storage), in both templates:
  - upload a png → metadata → content bytes and headers → list → delete → 404;
  - too large → 413;
  - a `.exe`/plain text → 415;
  - a type the client claims wrongly is ignored;
  - another user → 404;
  - an insert failing leaves no object;
  - a pdf is sent as an attachment.
- [ ] Gates pass, including both templates' checks and the drift test.
