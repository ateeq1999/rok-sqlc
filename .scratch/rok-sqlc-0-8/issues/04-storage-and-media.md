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

**Status:** resolved

- [x] PostgreSQL tests (memory storage), in both templates:
  - upload a png → metadata → content bytes and headers → list → delete → 404;
  - too large → 413;
  - a `.exe`/plain text → 415;
  - a type the client claims wrongly is ignored;
  - another user → 404;
  - an insert failing leaves no object;
  - a pdf is sent as an attachment.
- [x] Gates pass, including both templates' checks and the drift test.

## Comments

Done on `rok-sqlc-0-8-04`.

- **The port (`src/shared/storage/`, byte-identical, in the drift test's `SHARED`).** `#[async_trait] trait Storage: Send + Sync { put(&self, key, bytes::Bytes, content_type: &str) -> Result<(), AppError>; get(&self, key) -> Result<Option<StoredObject { bytes: Bytes, content_type: String }>, AppError>; delete(&self, key) -> Result<(), AppError> }`. `get` answers whole bytes, not a stream: uploads are capped (10 MiB media, 2 MiB avatars) and bytes keep the port free of either framework's body type. A missing key is `Ok(None)`; deleting a missing key is `Ok(())`; a store failure is `AppError::Internal`. `is_valid_key(&str)` (`/`-separated `[A-Za-z0-9._-]` segments, no `.`/`..`/empty) is checked by `put` (error), `get` (`None`) and `delete` (no-op). Keys are server-generated: media uses `media/<uuid simple>`; 05 should use `avatars/<random>.jpg` and `put(key, jpeg, "image/jpeg")`, `get` it for `GET /avatars/{file}` (validate `file` first), and `delete` the old key best effort (log, as `MediaService::forget` does).
- **The implementation (`object.rs`, `url.rs`).** `ObjectStorage` over `object_store` 0.13 (`aws` feature): `ObjectStorage::memory()`, and `storage::open(&StorageUrl) -> Result<Arc<dyn Storage>, object_store::Error>` for `file://<dir>` (created when missing; `file:///abs`, `file:///C:/x`, `file://./rel`), `s3://<bucket>` (`AmazonS3Builder::from_env()`, so `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_REGION`, `AWS_ENDPOINT`, `AWS_ALLOW_HTTP`) and `memory://`. S3 and memory keep the content type as an attribute; `LocalFileSystem` refuses attributes, so a file store re-detects the type with `infer` on `get` (`application/octet-stream` when unknown). `StorageUrl: FromStr + Display`, `StorageUrl::default_for_development()` = `file://./storage` (the templates' `.gitignore` gains `/storage`).
- **Config (`config.rs`, additive).** `storage_url: StorageUrl` (`STORAGE_URL`, invalid → `ConfigError::NotOneOf`), `media_max_upload_bytes: usize` (`MEDIA_MAX_UPLOAD_BYTES`, 10485760), `media_allowed_types: Vec<String>` (`MEDIA_ALLOWED_TYPES`, comma-separated, trimmed; default jpeg, png, webp, gif, pdf). `.env.example` documents them.
- **AppState.** Both gain `storage: Arc<dyn Storage>` and `AppState::with_storage(Arc<dyn Storage>) -> Self`. **Deviation:** `AppState::new` stays infallible, so it starts with `ObjectStorage::memory()`; `run` opens `STORAGE_URL` (`storage::open`, failing startup like the mailer) and calls `with_storage`. Tests therefore store in memory without configuring anything. actix also holds `media_max_upload_bytes`/`media_allowed_types` and builds `MediaService` in `configure`; axum holds `media: Arc<MediaService>` (built in `new`; `with_storage` updates it through `Arc::make_mut`). 05 can take `state.storage` (actix: register it or put it in `AccountService`; axum: `FromRef` already hands out `Arc<dyn Storage>`).
- **Errors.** New `AppError::UnsupportedMediaType(String)` → `415 unsupported_media_type` in both `error.rs`; `docs/guide/wire-standard.md`'s template table lists it. `413` is the existing `PayloadTooLarge` (`payload_too_large`).
- **Body limits.** actix: `media::routes(cfg, max_upload_bytes)` mounts a `web::scope("/api/v1/media")` whose `""` resource carries `web::PayloadConfig::new(max)`; the handler takes `Result<web::Bytes, actix_web::Error>` and maps a `413` error to `AppError::PayloadTooLarge` (actix's own is plain text), anything else to `400`. axum: `media::routes(max)` puts `.layer(DefaultBodyLimit::max(max))` on the `/api/v1/media` method router (overriding the router's global 2 MiB); the handler takes `Result<Bytes, BytesRejection>` and maps `rejection.status() == 413` the same way. Tested: 1025 bytes over a 1024 limit → JSON `413`, and 3 MiB under the default 10 MiB is stored, in both.
- **Feature `media`** (`features/media/`, `dto.rs`/`service.rs` byte-identical; `HAND_WRITTEN` in the drift test). `MediaService { pool, storage, max_upload_bytes, allowed_types }` (pub fields, `Clone`): `upload(UserId, Bytes, Option<String>)` (empty → `400`, over limit → `413`, name over 255 chars → `422`, `infer` → `415`; put, insert, delete the object if the insert fails), `list(UserId, Pagination)`, `metadata`, `content` (→ `MediaContent { bytes, content_type, disposition }`), `delete` (row, then object best effort). **Deviation:** the contract declares the file name as `?filename=` (query), the ticket as `X-Filename`; both are accepted, the query first. `content` also sends `Content-Security-Policy: default-src 'none'; sandbox`; `Content-Disposition` is `inline` for images, else `attachment`, with a sanitized name. Queries `queries/media.sql` → `db::queries::media::{insert_media, owned_media, list_owned_media, count_owned_media, delete_owned_media}`, all owner-scoped. No migration was needed (no `0005_media.sql`).
- **Contract.** `COVERED` gained the five media operations (separate lines after the invitations line); `openapi.rs` lists the five handlers after `invite`. No `KNOWN` lines needed.
- **Test helpers (`tests/support/mod.rs`, appended at the end, both templates).** `send(&AppState, token, Method, uri, &[(header, value)], Vec<u8>) -> Raw { status, headers: HashMap<lowercase name, value>, bytes }` with `Raw::json()` and `Raw::header(name)`, for raw bodies and non-JSON answers; `Recorded` (a `Storage` over memory that tracks live keys: `Recorded::new()`, `.keys()`), swapped in with `state(&pool)?.with_storage(Arc::new(recorded.clone()))`. `tests/media.rs` (9 tests, the same in both but for the test attribute): the ticket's list plus pagination, an empty body, a long name. The failed-insert test adds a `BEFORE INSERT` trigger raising an exception on `media` in the test schema.
- **Dependencies.** `bytes = "1"`, `infer = { version = "0.22", default-features = false }`, `object_store = { version = "0.13", features = ["aws"] }` in both `Cargo.toml.template`s and README blocks. `Cargo.lock`: actix +15 packages, axum +17 (object_store 0.13 shares reqwest 0.12; 0.14 would pull a second reqwest).
- READMEs (an Uploads section, the yours/shared lists), `AGENTS.md` (an Uploads section), `rok-sqlc.toml`'s exclude comment updated.
