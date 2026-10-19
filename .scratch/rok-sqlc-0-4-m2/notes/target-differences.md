# Generated document vs the target spec, shared CRUD paths

This is input for milestone 3's template and contract test. It was recorded by the milestone 2 spec review on 2026-10-19, which compared the document from an `init` + `g` project (users, media, api_keys, audit_log; scratch output `review-m2/gen.json`) with `../../rok-sqlc-0-4/notes/openapi.json`.

## Already matching

- prefix, users' methods and operation ids, tags;
- `type:[T,"null"]`, uuid and date-time formats, enum labels;
- `PaginatedResponse_*` fields, `page`/`per_page` as int32 with minimum 0;
- security per operation.

## Differences

### Errors

- Every 4xx has `content: ErrorBody`, but the target's error responses have no content. The target documents no error schema at all. Decide whether the contract test ignores error bodies, or whether generation leaves them out.
- The generated document adds 400 and 401 everywhere, 403 on admin routes, 409 on update and remove, and 413/422 on bodies. These are real answers the handlers give.
- `/health/ready`'s 503 has a body.

### Metadata the generated document adds

- `summary` on every operation;
- `description`s on params and fields;
- a `Location` header on 201.

### Names

- The enum is `UserRole`; the target has `Role`. This is the enum's Rust name; a value-list enum is named after table plus column.
- `PaginatedResponse_AuditLogResponse` vs `PaginatedResponse_AuditLogEntryResponse`.
- `total` has no `minimum: 0`.
- The target's `UserSort` and `SortOrder` are dangling refs; the generated ones are defined.

### users

- `UserResponse` adds `updated_at` and `deleted_at`, and lacks the computed `has_password` and `avatar_url`.
- `CreateUserDto` and `UpdateUserDto` list every column, hidden and server-set ones included, with `default: null`. The target has a `password` field and only 3 updatable fields. Conventions (managed columns) and hand-written DTOs in the template should close this.
- DELETE adds a `permanent` query param on soft-delete tables.

### media

- POST is `create` with a JSON `CreateMediaDto`; the target's is `upload`, binary, with `?filename`, a hand-written upload.
- GET is `get_one`; the target's is `metadata`.
- The list adds `q`, `sort` and `order`.
- `MediaResponse` has `owner_id` but no `url`.

### api-keys

- The list is paginated; the target returns a bare array.
- Create returns `ApiKeyResponse`; the target returns `CreatedApiKeyResponse`.
- DELETE is `remove`; the target's is `revoke`.
- The DTO and response differ: ours have `key_hash`, `user_id` and `revoked_at`, but no `expires_in_days`.

### audit-log

- `details` isn't required.
- The list adds `q`, `sort` and `order`.

### Other

- The path param description is "The `id` column." where the target says "User id".
- Extra paths: by-email, restore, and the relationship routes.
