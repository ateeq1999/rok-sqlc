# Files and storage (the templates)

The templates (`rok-sqlc init --template actix` or `axum`) store files two
ways: **uploads** (`/api/v1/media`, the hand-written `media` feature), any
allowed file a signed-in user keeps, and **profile photos**
(`/api/v1/users/me/avatar`, in the `account` feature), one small JPEG per
user. Both store their bytes through one port, `Storage`, chosen by
`STORAGE_URL`. The decisions are in ADR 0020
(`docs/adr/0020-two-factor-api-keys-and-files.md`); the accounts and API
keys these routes check are on [Accounts and sign-in](auth.md).

- [Where the code is](#where-the-code-is)
- [Uploads](#uploads)
- [Profile photos](#profile-photos)
- [Storage](#storage)
- [S3 and S3-compatible stores](#s3-and-s3-compatible-stores)
- [Settings](#settings)
- [Limits](#limits)
- [Testing](#testing)

## Where the code is

| Path | What it holds |
|---|---|
| `src/features/media/` | the uploads: list, upload, metadata, content, delete |
| `src/features/account/` | the profile photo: set, remove, and the public `GET /avatars/{file}` |
| `src/shared/storage/` | the `Storage` port and `ObjectStorage`, its implementation over `object_store`; `StorageUrl` |
| `src/shared/thumbnail.rs` | decoding a photo under limits, cropping it, re-encoding it as JPEG |
| `src/shared/body.rs` | reading an upload's body, and answering the wire standard's `413` |
| `queries/media.sql`, `queries/account.sql` | the features' SQL |
| `migrations/0001_init.sql`, `0005_avatars.sql` | the `media` table; `users.avatar_url` |

`dto.rs` and `service.rs` of both features, `shared/storage/` and
`shared/thumbnail.rs` are the same file in both templates; handlers,
routes and `shared/body.rs` are written for the framework.

## Uploads

Every route needs a signed-in caller (a session or an API key), and every
file is its owner's alone: another user's file is a `404`, as a missing
one is.

| Endpoint | Answers |
|---|---|
| `POST /media` | `201` the file's metadata; `400`, `413`, `415`, `422` |
| `GET /media` | `200` the caller's files, newest first (`?page=&per_page=`) |
| `GET /media/{id}` | `200` the metadata, `404` |
| `GET /media/{id}/content` | `200` the bytes, `404` |
| `DELETE /media/{id}` | `204`, `404` |

**Uploading.** The body is the raw file, not a form: its `Content-Type` is
ignored. Name it with `?filename=` or the `X-Filename` header (255
characters at most, `422` past it); the query wins when both are sent.

```sh
curl -X POST "$API/api/v1/media?filename=report.pdf" \
  -H "Authorization: Bearer $TOKEN" --data-binary @report.pdf
```

```jsonc
{
  "id": "01a1...",
  "content_type": "application/pdf",
  "size_bytes": 48213,
  "original_filename": "report.pdf",
  "url": "/api/v1/media/01a1.../content",
  "created_at": "2026-10-23T08:00:00Z"
}
```

An empty body is a `400`, one over `MEDIA_MAX_UPLOAD_BYTES` a `413`. The
type is detected from the file's first bytes (`infer`), never from its
name or the request's headers, and must be one of `MEDIA_ALLOWED_TYPES`:
otherwise `415 unsupported_media_type`
([wire standard](wire-standard.md#415-unsupported-media-type-the-templates)).
The bytes are stored first, under a key the server makes
(`media/<uuid>`), then the row; if the row fails, the bytes are deleted.

**Serving.** `GET /media/{id}/content` answers the bytes with the detected
`Content-Type`, `X-Content-Type-Options: nosniff` (so a browser never
guesses another type) and `Content-Security-Policy: default-src 'none';
sandbox` (so a document opened anyway runs nothing). An image is served
`Content-Disposition: inline`; anything else `attachment`, with a cleaned
file name, so it downloads instead of opening in your origin.

**Deleting** removes the row, then the bytes. A store that fails to delete
them is logged, not answered: the row is gone, and the orphaned object can
be cleaned up later.

## Profile photos

`PUT /users/me/avatar` takes a raw JPEG, PNG or WebP image of at most 2 MiB
and answers the updated user (`200`). The image is decoded under limits
(at most 8192×8192 pixels and 128 MiB of memory, so a small file claiming
a huge size is refused before it is allocated), cropped to its centre,
scaled to 256×256, transparency flattened onto white, and re-encoded as a
JPEG: EXIF, GPS positions and anything else the original carried are gone.
A file that isn't one of those formats, can't be decoded, or is over the
limits is a `422`; over 2 MiB, a `413`. GIF isn't accepted: one frame of
an animation is not what a user picks.

The JPEG is stored as `avatars/<random>.jpg`, and the previous one is
deleted once the new one is committed. `DELETE /users/me/avatar` removes
it. Every user response answers `avatar_url`: `/api/v1/avatars/<file>`,
or `null`. That route is public (an `<img>` tag sends no token) and
answers `image/jpeg` with `Cache-Control: public, max-age=31536000,
immutable`: a new photo gets a new name, so a cached one is never stale.
The name is random, so it can't be guessed from the user's id.

`avatar_url` is a generated column of `users`
(`'/api/v1/' || avatar_key`, in `0005_avatars.sql`), so the generated
`users` feature answers it too. Moving the API off `/api/v1` means changing
that column as well.

## Storage

Features store bytes through the `Storage` port in `src/shared/storage/`,
an `async_trait` with three methods: `put(key, bytes, content_type)`,
`get(key)`, answering `Option<StoredObject { bytes, content_type }>`, and
`delete(key)`. A missing key is `Ok(None)`, deleting one is no error, and a store that
fails is an `AppError::Internal` (a `500`, the cause logged). Keys are made
by the server, never taken from a request, and `is_valid_key` refuses any
that could leave the store (`..`, empty segments, anything but
`A-Za-z0-9._-` and `/`). `get` answers the whole object rather than a
stream: every upload is capped, and bytes keep the port free of either
framework's body type.

`ObjectStorage` implements it over the `object_store` crate, chosen by
`STORAGE_URL`:

| `STORAGE_URL` | Where the bytes go |
|---|---|
| `file://./storage` (the default) | a `storage/` directory under the working directory, created when missing |
| `file:///srv/uploads`, `file:///C:/uploads` | an absolute directory |
| `s3://<bucket>` | an S3 bucket, or any S3-compatible store ([below](#s3-and-s3-compatible-stores)) |
| `memory://` | memory, lost when the process stops: for tests |

`AppState::new` opens the store, so every application built from it
stores where it is configured; a store that can't be opened (a `file://`
path that is a file, say) stops startup
([troubleshooting](troubleshooting.md#running-the-application)). A local
store keeps no content type, so it is detected again from the bytes when a
file is read; S3 and memory keep the one it was put with. To store
somewhere else, implement `Storage` and pass it to
`AppState::with_storage`.

Add `/storage` to your `.gitignore` (the templates do) and to your backups.

## S3 and S3-compatible stores

Set `STORAGE_URL=s3://<bucket>`; the rest comes from the standard `AWS_*`
variables (`object_store`'s `AmazonS3Builder::from_env`):

```sh
STORAGE_URL=s3://my-app-uploads
AWS_REGION=eu-west-1
AWS_ACCESS_KEY_ID=...
AWS_SECRET_ACCESS_KEY=...
```

For an S3-compatible store (MinIO, Cloudflare R2, Backblaze B2, a local
MinIO in development), add its endpoint, and `AWS_ALLOW_HTTP=true` for one
without TLS:

```sh
STORAGE_URL=s3://uploads
AWS_ENDPOINT=http://localhost:9000
AWS_ALLOW_HTTP=true
AWS_REGION=us-east-1
AWS_ACCESS_KEY_ID=minioadmin
AWS_SECRET_ACCESS_KEY=minioadmin
```

The bucket isn't reached at startup: wrong credentials or a wrong region
show on the first upload, as a `500` with the cause in the log. Give the
credentials only `s3:PutObject`, `s3:GetObject` and `s3:DeleteObject` on
the bucket. Keep the bucket private: every file is served through the API,
which checks its owner.

## Settings

| Variable | Default | Setting |
|---|---|---|
| `STORAGE_URL` | `file://./storage` | where uploaded bytes go (see [Storage](#storage)) |
| `MEDIA_MAX_UPLOAD_BYTES` | `10485760` (10 MiB) | the largest upload, in bytes |
| `MEDIA_ALLOWED_TYPES` | `image/jpeg,image/png,image/webp,image/gif,application/pdf` | the types an upload may have, detected from its bytes, comma-separated |

`.env.example` lists them; `src/config.rs` reads them. An invalid
`STORAGE_URL` or a number that isn't one stops startup with one line on
stderr.

## Limits

| What | Limit | Past it |
|---|---|---|
| An upload | `MEDIA_MAX_UPLOAD_BYTES` (10 MiB) | `413 payload_too_large` |
| A profile photo | 2 MiB | `413 payload_too_large` |
| A photo's pixels | 8192×8192, 128 MiB to decode | `422 validation_failed` |
| A file name | 255 characters | `422 validation_failed` |
| A JSON body | 2 MiB | `413 payload_too_large` |

A body is read only once the caller is authenticated, and only up to its
route's limit. Under actix-web, the upload handlers take `web::Payload` and
read it with `shared::body::read` in their body: actix polls a handler's
extractors together, so a `web::Bytes` parameter would buffer the body
while `AuthUser` was still checking the credential. Under axum, extractors
run in order, so the body (`Bytes`, last) is read after `AuthUser`; each
upload route has its own `DefaultBodyLimit` in place of the router's
2 MiB. Your proxy's own limit (nginx's `client_max_body_size`, 1 MiB by
default) must be at least `MEDIA_MAX_UPLOAD_BYTES`.

## Testing

The tests configure `STORAGE_URL=memory://` (`tests/support::config`), so
they never touch the disk. `tests/support::Recorded` is a `Storage` that
records which keys hold an object, swapped in with
`state(&pool)?.with_storage(Arc::new(recorded.clone()))`, for a test that
checks what was stored or deleted; `tests/media.rs` and
`tests/avatars.rs` use it. `support::send` sends a raw body with any
headers and answers the status, headers and bytes.
