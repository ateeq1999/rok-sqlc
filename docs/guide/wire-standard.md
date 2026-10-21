# The wire standard

Every generated API answers the same way, under actix-web or axum: the
same success shapes, the same error body, the same list shape and the same
headers. This page lists all of them; [What the frameworks
add](#what-the-frameworks-add) lists the few answers only one gives. The exchanges are real answers of
[`docs/examples/api`](../examples/api) (with the `date` and
`content-length` headers left out), and the Rust samples come from its
source.

## Success

A success answers the bare object, never an envelope:

| Route | Status | Body | Headers |
|---|---|---|---|
| `GET /` (list) | `200` | a page: `{items, page, per_page, total}` | |
| `POST /search` | `200` | a page, like the list | |
| `POST /` (create) | `201` | the row | `Location`, the row's URL; `ETag` on a versioned table |
| `GET /{id}` | `200` | the row | `ETag` on a versioned table |
| `PATCH /{id}` | `200` | the row, changed | `ETag` on a versioned table |
| `DELETE /{id}` | `204` | none | |
| `POST /{id}/restore` | `200` | the row, restored | `ETag` on a versioned table |
| `GET /by-<column>/{value}` | `200` | the row | `ETag` on a versioned table |
| `GET /{id}?include=<name>` | `200` | the row's fields and, under `<name>`, the related row or `null` | `ETag` on a versioned table |
| `GET /{id}/<name>` | `200` | the related rows, a bare JSON array | |
| `PUT /{id}/<name>/{other}`, `DELETE /{id}/<name>/{other}`, `PUT /{id}/<name>` | `204` | none | |

A created row:

```http
HTTP/1.1 201 Created
content-type: application/json
location: /api/v1/projects/01a1538f-54cf-729e-b214-0e47a960825b

{"id":"01a1538f-54cf-729e-b214-0e47a960825b","owner_id":"0190a4b2-4c1e-7d3a-9f00-000000000001","name":"Launch","open_tasks":0,"created_at":"2026-10-19T09:47:44.718664Z"}
```

A row's JSON has one field per column, named as the column, less the
`[api.hidden]` columns. Timestamps are RFC 3339 in UTC, UUIDs strings, and
a `NULL` is `null`.

## Lists

`GET /` takes `?page=&per_page=&q=&sort=&order=`:

| Parameter | Default | Meaning |
|---|---|---|
| `page` | `1` | the page, from 1; `0` is 1 |
| `per_page` | `20` | the rows a page holds, clamped to 1 to 100 |
| `q` | none | text the `[api.search]` columns are matched against (`ILIKE`, any column); only on a table that has them |
| `sort` | `created_at`, or the first sort key | one of the table's sort keys (`[api.sort]`) |
| `order` | `desc` | `asc` or `desc` |

It answers one page, with the total over every page:

```http
GET /api/v1/projects?page=2&per_page=1

HTTP/1.1 200 OK
content-type: application/json

{"items":[{"id":"01a1538f-54cf-729e-b214-0e47a960825b","owner_id":"0190a4b2-4c1e-7d3a-9f00-000000000001","name":"Launch","open_tasks":0,"created_at":"2026-10-19T09:47:44.718664Z"}],"page":2,"per_page":1,"total":2}
```

<!-- source: docs/examples/api/src/shared/response.rs -->
```rust
/// One page of a list: `{"items", "page", "per_page", "total"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PaginatedResponse<T> {
    /// The page's rows.
    pub items: Vec<T>,
    /// The page, from 1.
    pub page: u32,
    /// The most rows a page holds.
    pub per_page: u32,
    /// The rows on every page together.
    #[schema(minimum = 0)]
    pub total: i64,
}
```

Pages are offsets, so a row added while a client pages through may shift
the next page by one. Ties in the sort are broken by the primary key, so
the order is stable. `POST /search` (with `[api] search_endpoint`) takes
the same `sort`, `order`, `page` and `per_page` in its body, beside a
`filter` ([Configuring](configuring.md#search_endpoint)), and answers the
same page.

## Errors

Every error answers the same body: a snake_case `code` to branch on, a
`message` for people, and, for broken validation rules, `details` by
field.

```jsonc
{"error": {"code": "not_found", "message": "no `projects` row has this key"}}
```

| Code | Status | When |
|---|---|---|
| `bad_request` | 400 | the body isn't JSON or isn't sent as `Content-Type: application/json`; the path or the query string doesn't parse (a malformed UUID, `?page=abc`, an unknown `sort`) |
| `unauthorized` | 401 | no bearer token, or one that is invalid or expired |
| `forbidden` | 403 | an `admin` route called with a token whose role isn't `admin` |
| `not_found` | 404 | no row has the key, or it is another caller's (`[api.owner]`) |
| `conflict` | 409 | a unique or foreign-key violation, naming the constraint (a race the `exists` or `unique` check missed, for one) |
| `version_conflict` | 412 | `If-Match` holds a stale version, or none |
| `payload_too_large` | 413 | the body is over the JSON limit: 2 MiB (actix-web's default, unless you set a `JsonConfig`; axum's `DefaultBodyLimit` in `app.rs`) |
| `validation_failed` | 422 | JSON that doesn't fit the body (a wrong type, a missing field), a body breaking its rules, a foreign key naming no row the caller can see (`exists`), a value a unique key already holds (`unique`), or a check constraint violation |
| `precondition_required` | 428 | `PATCH` on a versioned table without `If-Match` |
| `internal_error` | 500 | anything else, such as a lost database connection |

A generated route's OpenAPI document lists exactly the statuses it can
answer, each with this body (`ErrorBody`), except `500`, which any route
can answer.

The scaffolded `src/error.rs` maps each `AppError` variant to its code and
status. It is yours: add a variant there for a new error case.

The templates (`init --template actix` or `axum`) add four for their
account flows:

| Code | Status | When |
|---|---|---|
| `invalid_token` | 400 | a mailed link's token is unknown, expired or already used |
| `wrong_password` | 400 | the current password a flow asks for again doesn't match |
| `breached_password` | 400 | a new password appears in a known data breach (`BREACH_CHECK=true`) |
| `too_many_requests` | 429 | over the `/auth` rate limit, `AUTH_RATE_LIMIT` requests a minute per IP; `Retry-After` gives the seconds to wait |

These `400`s are flow failures. A body of the wrong shape (an email that
isn't one, a password outside 8 to 128 characters) is still a `422
validation_failed`, and a sign-in with bad credentials a `401`.

<!-- source: docs/examples/api/src/error.rs -->
```rust
    pub fn code(&self) -> &'static str {
        match self {
            AppError::BadRequest(_) => "bad_request",
            AppError::Unauthorized(_) => "unauthorized",
            AppError::Forbidden(_) => "forbidden",
            AppError::NotFound(_) => "not_found",
            AppError::Conflict(_) => "conflict",
            AppError::PayloadTooLarge(_) => "payload_too_large",
            AppError::Validation { .. } => "validation_failed",
            AppError::PreconditionRequired(_) => "precondition_required",
            AppError::VersionConflict(_) => "version_conflict",
            AppError::Internal(_) => "internal_error",
        }
    }
```

### 400: malformed requests

```http
HTTP/1.1 400 Bad Request
content-type: application/json

{"error":{"code":"bad_request","message":"Path deserialize error: UUID parsing failed: invalid character: found `n` at 0"}}
```

```http
HTTP/1.1 400 Bad Request
content-type: application/json

{"error":{"code":"bad_request","message":"Query deserialize error: unknown variant `owner_id`, expected `id` or `created_at`"}}
```

```http
HTTP/1.1 400 Bad Request
content-type: application/json

{"error":{"code":"bad_request","message":"the request body is not valid JSON: EOF while parsing a value at line 1 column 8"}}
```

### 400: flow failures (the templates)

A well-formed request that an account flow refuses answers `400` with the
flow's own code, never details, so a client can tell it from a body to
fix (`422`):

```http
HTTP/1.1 400 Bad Request
content-type: application/json

{"error":{"code":"invalid_token","message":"the link is invalid, expired or already used"}}
```

```http
HTTP/1.1 400 Bad Request
content-type: application/json

{"error":{"code":"wrong_password","message":"the current password is wrong"}}
```

```http
HTTP/1.1 400 Bad Request
content-type: application/json

{"error":{"code":"breached_password","message":"this password appears in a known data breach; choose another"}}
```

`invalid_token` doesn't say which of unknown, expired or used: ask for a
new link. A password reset checks its link before the new password, so a
`breached_password` or a `422` leaves the link usable.

### 429: too many requests (the templates)

Every request under `/api/v1/auth` counts against `AUTH_RATE_LIMIT`
requests a minute per client IP (20 by default), an unknown path or
method there too (answered `404 not_found`). Past it, the answer says how many seconds to
wait, in the `Retry-After` header and the message:

```http
HTTP/1.1 429 Too Many Requests
retry-after: 3
content-type: application/json

{"error":{"code":"too_many_requests","message":"too many requests; retry in 3s"}}
```

### 401 and 403: the caller

```http
HTTP/1.1 401 Unauthorized
content-type: application/json

{"error":{"code":"unauthorized","message":"the token is invalid or expired"}}
```

Without a token the message is `a bearer token is required`, and an admin
route called by a user answers `403` with `this needs the admin role`.
These messages come from your `src/shared/auth.rs`. In the actix template, a valid
token whose account is deleted or disabled, or was signed out by a password
change or reset, answers `401` with `the token is no longer valid`.

### 404: no such row

Another caller's row on an owned table answers exactly like a missing one,
so a caller can't learn which keys exist:

```http
HTTP/1.1 404 Not Found
content-type: application/json

{"error":{"code":"not_found","message":"no `projects` row has this key"}}
```

### 409: constraints

```http
HTTP/1.1 409 Conflict
content-type: application/json

{"error":{"code":"conflict","message":"the change breaks a reference (constraint `tasks_project_id_fkey`)"}}
```

That is a project deleted while tasks still reference it. A unique
violation reads ``the value is already taken (constraint `<name>`)``; the
generated writes check unique keys first (`unique`, below), so it is the
answer of a race between two writes, or of a key the check leaves out. Name
your constraints in the migrations if clients branch on them.

### 422: validation

A body breaking its rules answers `details`, validator's errors by field.
The rules come from the schema (`VARCHAR(80)` gives a length rule, a
`CHECK (open_tasks >= 0)` a range rule):

```http
HTTP/1.1 422 Unprocessable Entity
content-type: application/json

{"error":{"code":"validation_failed","message":"the request body failed validation","details":{"open_tasks":[{"code":"range","message":"open_tasks must be at least 0","params":{"min":0,"value":-1}}]}}}
```

A `PATCH` body naming an immutable column (`[api.immutable]`), even as
`null`, breaks the rule `immutable` under that field. The field is never
ignored, though fields the body doesn't know are:

```http
HTTP/1.1 422 Unprocessable Entity
content-type: application/json

{"error":{"code":"validation_failed","message":"the request body failed validation","details":{"email":[{"code":"immutable","message":"`email` can't be changed","params":{"value":"b@example.com"}}]}}}
```

A foreign key in a create or update body must name a row the caller can
see: one that exists, isn't soft-deleted, is in the caller's tenant and,
when the referenced table is owned (`[api.owner]`) and the route has a
caller, is the caller's. The generated `create_in` and `update_in` check
every such field (a `null` or absent one is skipped) on the write's
transaction before writing, and answer every miss at once, under the code
`exists`:

```http
HTTP/1.1 422 Unprocessable Entity
content-type: application/json

{"error":{"code":"validation_failed","message":"the request failed validation","details":{"project_id":[{"code":"exists","message":"project_id does not refer to an existing project","params":{}}]}}}
```

A value a unique key of the table already holds fails too, under the code
`unique`, in the same answer as the `exists` misses. Every unique key whose
columns the body holds is checked, the tenant and owner columns taken from
the caller; a composite key is reported against its last column:

```http
HTTP/1.1 422 Unprocessable Entity
content-type: application/json

{"error":{"code":"validation_failed","message":"the request failed validation","details":{"email":[{"code":"unique","message":"email is already taken","params":{}}],"seat":[{"code":"unique","message":"region and seat are already taken together","params":{}}]}}}
```

The check counts the rows the key does: only live ones for a partial
index on `<soft-delete column> IS NULL`, and only the caller's tenant's
(and the caller's own, for a key with the owner column). An update never
counts the row it changes, so sending a row's own value back passes. A
key's `lower(<column>)` entry compares that column ignoring case.

What the pre-checks leave to the constraint answers `409 conflict`, as
[above](#409-constraints): a row deleted or a duplicate written between
the check and the write, a reference from a public route to an owned
table (no caller to scope it to), a reference to anything but the
referenced table's whole single-column key, and a `PATCH` holding only
some of a composite key's columns. The
[configuring guide](configuring.md#pre-checks-from-the-schema-unique-and-exists)
lists every case.

Every detail code a generated API answers:

| Code | From | Meaning |
| --- | --- | --- |
| `length` | the schema (`VARCHAR(n)`) | longer than the column takes |
| `range` | the schema (a `CHECK` bounding a number, such as `stock >= 0`) | outside the bounds the check sets |
| `immutable` | `[api.immutable]` | a `PATCH` names a column set only on create |
| `exists` | foreign keys | the row named isn't one the caller can see |
| `unique` | unique keys | another row already holds the value |
| `email`, `url`, `uuid`, `ulid`, `ipAddress`, `mobile`, `alpha`, `alphaNumeric`, `ascii`, `regex`, `in`, `notIn`, `startsWith`, `endsWith`, `minLength`, `maxLength`, `fixedLength`, `hexCode`, `iban`, `jwt`, `postalCode`, `creditCard`, `coordinates` | `[api.rules]` | the [string rule](configuring.md#rules) of that name failed |

A hand-written request type adds its own codes, such as `validator`'s.

JSON that doesn't fit the body answers serde's message, without details:

```http
HTTP/1.1 422 Unprocessable Entity
content-type: application/json

{"error":{"code":"validation_failed","message":"invalid type: integer `5`, expected a string at line 1 column 9"}}
```

A check constraint the rules don't cover is caught by the database and
answers ``the value breaks the check `<name>` ``, without details.

### 413: too large

```http
HTTP/1.1 413 Payload Too Large
content-type: application/json

{"error":{"code":"payload_too_large","message":"the request body is over the size limit"}}
```

### 500: everything else

The cause is logged with `tracing` and never sent; the message is always
`something went wrong`. A tenant-scoped route reached without a tenant (a
missing middleware) is one, since it is a wiring bug, not the caller's
fault.

Under axum, a route whose path lacks the parameters its handler takes
(a wiring bug too) answers this `500`, its detail logged.

## Versioned rows

On a table with a version column (`[conventions] version_column`), every
answer holding one row carries the version as an `ETag`, in quotes:

```http
HTTP/1.1 200 OK
content-type: application/json
etag: "3"
```

`PATCH` needs it back in `If-Match`, exactly as received:

| `If-Match` | Answer |
|---|---|
| missing | `428 precondition_required`, `` `If-Match` must hold the row's `ETag` `` |
| not a quoted version (`*`, `W/"3"`, `3`) | `412 version_conflict`, `` `If-Match` holds no version of the row `` |
| an older version | `412 version_conflict`, `the row has changed since this version` |
| the current version | `200`, the changed row with its new `ETag` |

On a `412`, read the row again and retry with its new `ETag`.

## Health

`/health/live` and `/health/ready` sit outside the API prefix and need no
token:

| Route | Answer |
|---|---|
| `GET /health/live` | `200 {"status":"ok"}` while the process answers |
| `GET /health/ready` | `200 {"status":"ready"}` when the database answers `SELECT 1`, else `503` with code `service_unavailable` |

[Deploying](deploying.md#health-checks) says which to point each probe at.

## What the frameworks add

The answers above are the same under both frameworks, messages included
(axum's path and query failures are worded as actix-web words them).
What each framework adds around them differs (ADR 0019):

| | actix-web | axum |
|---|---|---|
| A request over 30 seconds | no timeout (set one in your proxy) | `408 request_timeout`, the error body |
| `x-request-id` | not set | set on every answer (and kept from the request) |
| CORS | none | same-origin (`CorsLayer::new()`) |
| An unknown route | `404`, empty body | `404`, empty body |
| A wrong method on a known path | `404`, empty body | `405`, empty body |
| Either, under `/api/v1/auth` (the templates) | `404 not_found` | `404 not_found` |

```http
HTTP/1.1 408 Request Timeout
content-type: application/json

{"error":{"code":"request_timeout","message":"the request took longer than 30s"}}
```

## Routes

Every feature serves its routes under `[api] prefix` (`/api/v1`) and its
route base, a plural kebab-case name (`/projects`, `/audit-log`). Keys are
path segments (`/{id}`; a composite key is one segment per column), an
update is `PATCH` with only the fields to change, and `null` sets a
nullable column to `NULL`. The README's [route
table](../../README.md#generated-apis) lists every route and the `[api.skip]`
name that drops it.
