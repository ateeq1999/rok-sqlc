# Configuring API features

The `[api]` section of `rok-sqlc.toml` decides which tables get an API
feature and how each behaves: who may call it, whose rows a caller sees,
what a response leaves out, and what a list searches and sorts by. This
page goes through every key, with what it changes in the routes and in the
OpenAPI document. Run `rok-sqlc g` after changing any of them.

The [README's configuration reference](../../README.md#rok-sqlctoml) lists
every key in one block. The same keys work in the JSON payload (`-j`), and
the layers merge as the README describes: `[api]`'s single keys and
`exclude` like the top-level keys, its table-keyed sections table by table.

## Table keys

Every section below `[api]` is keyed by table. A key may be the table's
SQL name (`audit_log`), its schema-qualified name (`"audit.log"`) or its
module name (after a `[modules]` rename). Each table it names must be
generated and each column must exist, or the run fails naming the entry:

```text
Error: `api.owner.posts` in the configuration names no generated table of the schema
```

[Troubleshooting](troubleshooting.md#configuration-errors) lists every such
error.

## `output`

```toml
[api]
output = "src/features"
```

The directory the features are generated into, one subdirectory per table.
Without it there is no API: every other `[api]` key is ignored, and `g`
generates only the data layer. `rok-sqlc init` sets it, and `-a`/
`--api-output` sets it from the command line.

It needs `derive = ["serde", "validate"]` and `actix_web` in `extractors`,
since the features take validated JSON bodies through the actix-web
extractor; `init` sets those too. With `output`, a run also generates
`db/pool.rs` in the data layer and the shared tools in `shared/`, the
directory beside `output` (`src/shared/`).

Join tables, which the data layer turns into a many-to-many relationship
between the two tables they pair, get no feature of their own: the two
features serve the relationship.

## `db_path`

```toml
[api]
output = "src/features"
db_path = "crate::db"
```

The Rust path of the generated data layer, which the features import
(`crate::db` by default). Change it only when `output` (the data layer's
directory) is not `src/db`. `check-arch` reads it too, and it must start
with `crate::`.

## `prefix`

```toml
[api]
output = "src/features"
prefix = "/api/v2"
```

The URL prefix of every feature's routes, `/api/v1` by default. It changes
the scopes in each `routes.rs`, the `Location` of a created row, and every
path in the OpenAPI document. `/health/live`, `/health/ready`, `/docs/` and
`/api-docs/openapi.json` stay outside it.

## `routes`

```toml
[api]
output = "src/features"

[api.routes]
audit_log = "/audit-log"
```

A table's route base. Without an entry it is `/` and the table's module
name in kebab case: `/projects` for `projects`, `/audit-log` for
`audit_log`. A base must start with `/` and be unique among the features'
bases, configured or default. The OpenAPI tag is the route base without its
`/`.

## `exclude`

```toml
[api]
output = "src/features"
exclude = ["audit_log", "sessions"]
```

Tables that get no feature. They stay in the data layer, so services can
still read and write them through `crate::db`, and relationship routes to
them are not generated.

## `access`

```toml
[api]
output = "src/features"

[api.access]
products = "public"
orders   = "user"
users    = "admin"
```

Who may call a table's routes:

| Value | Handlers take | Without it |
|---|---|---|
| `public` | no caller | anyone may call |
| `user` (the default) | `AuthUser` | `401 unauthorized` without a valid bearer token |
| `admin` | `AdminUser` | `401` without a valid token, `403 forbidden` for a token whose `role` isn't `admin` |

`AuthUser` and `AdminUser` live in the scaffolded `src/shared/auth.rs`,
which is yours: change how tokens are verified there. In the document, a
route needing a caller carries `security(("bearer_auth" = []))` and lists
`401` (and `403` for `admin`). A relationship route between two features
takes the stricter of their two accesses.

## `owner`

```toml
[api]
output = "src/features"

[api.owner]
projects = "owner_id"
tasks    = "owner_id"
```

An owner column scopes every row to the caller who owns it. It must be
`UUID NOT NULL`, and an owned table can't be `public`. On an owned table:

- every read and write matches the caller's id: a list holds only the
  caller's rows, and another caller's row is a `404`, as if it didn't
  exist;
- `create` sets the owner to the caller, so neither body has the column;
- `update` and `remove` first lock the caller's row (`SELECT … FOR
  UPDATE`), then change it;
- every service method takes the caller, `user: AuthUser`.

The example owns both its tables. Its create body has no `owner_id`, and
`into_new` takes the owner from the caller:

<!-- source: docs/examples/api/src/features/projects/dto.rs -->
```rust
impl CreateProjectDto {
    /// The [`NewProject`] of this body, owned by `owner`: the caller's id.
    #[must_use]
    pub fn into_new(self, owner: uuid::Uuid) -> NewProject {
        NewProject {
            id: None,
            owner_id: owner,
            name: self.name,
            open_tasks: self.open_tasks,
            created_at: self.created_at,
        }
    }
}
```

Rows of an owned table reached through a relationship are filtered to the
caller too: an embedded row the caller doesn't own is `null`, and it is
left out of a related list.

## `hidden`

```toml
[api]
output = "src/features"

[api.hidden]
users = ["password_hash", "deleted_at"]
```

Columns left out of the table's response, `UserResponse`, and so out of
every answer and of the document's response schema. A hidden column is
still in the create and update bodies, unless a convention or `owner`
leaves it out; for a secret like a password hash, set it in a service
override rather than taking it from the body (see
[Customizing](customizing.md)).

A key column can't be hidden. `[api.search]` and `[api.sort]` may not name
a hidden column, it gets no `/by-<column>` route, and a `/search` filter on
it is a `422 validation_failed`.

## `immutable`

```toml
[api]
output = "src/features"

[api.immutable]
users    = ["email"]
projects = ["slug"]
orders   = ["order_number", "currency"]
payments = ["external_id"]
```

Columns a caller sets when creating a row and never changes through the
API: a slug that is in URLs, an order number, an id from another system,
the currency an amount was taken in, or an email that only a verification
flow may change. An immutable column stays in the create body
(`CreateUserDto`), required when it is `NOT NULL` without a default, and in
every response. It leaves the update body: `UpdateUserDto` has no `email`
in the document, and a `PATCH` naming it, even as `null`, answers
`422 validation_failed` with the details under the field rather than
ignoring it:

```http
HTTP/1.1 422 Unprocessable Entity
content-type: application/json

{"error":{"code":"validation_failed","message":"the request body failed validation","details":{"email":[{"code":"immutable","message":"`email` can't be changed","params":{"value":"b@example.com"}}]}}}
```

The data layer's patch, `UpdateUser`, keeps the column, so your own code
(a service override, a verified email change) can still write it.

A key, owner or tenant column can't be immutable (no update changes it
anyway), nor a managed or generated one (generated code never writes it).

## `rules`

```toml
[api]
output = "src/features"

[api.rules.users]
email   = ["trim", "toLowerCase", "email"]
phone   = ["mobile(ar-SA, ar-AE)"]
website = ["url"]
handle  = ["alphaNumeric(allowDashes)", "minLength(3)", { regex = "^[a-z0-9-]+$" }]
country = ["in(SA, AE, EG)"]
```

String rules and transforms for text columns, by
[VineJS's names](https://vinejs.dev/docs/types/string). A rule is a string,
with its arguments in parentheses, or an inline table where a string can't
carry the argument: `{ regex = "..." }`, and `{ in = ["a,b", "c"] }`,
`{ notIn = [...] }`, `{ startsWith = "a, b" }` and `{ endsWith = "..." }`
for values with commas. Both bodies, `CreateUserDto` and `UpdateUserDto`,
check them; a failed rule answers `422 validation_failed` under the field,
with the rule's name as the detail code and an English message naming the
column:

```http
HTTP/1.1 422 Unprocessable Entity
content-type: application/json

{"error":{"code":"validation_failed","message":"the request body failed validation","details":{"country":[{"code":"in","message":"country must be one of SA, AE, EG","params":{"value":"US"}}]}}}
```

| Rule | Checks |
| --- | --- |
| `email`, `url`, `ipAddress`, `ipAddress(4)`, `ipAddress(6)` | `validator`'s own `email`, `url` and `ip` rules (a URL needs a scheme) |
| `uuid`, `uuid(4, 7)` | a UUID of any version 1 to 8 (or nil or max), or of the versions named |
| `ulid` | 26 Crockford base32 characters, the first at most `7` |
| `mobile(ar-SA, ...)`, `mobile(..., strictMode)` | validator.js's pattern of each locale named; `strictMode` needs a leading `+` |
| `alpha`, `alphaNumeric` | ASCII letters (and digits), with `allowSpaces`, `allowDashes`, `allowUnderscores` |
| `ascii` | one or more ASCII characters |
| `{ regex = "..." }` | matches the pattern, in the `regex` crate's syntax |
| `in(...)`, `notIn(...)` | is (or isn't) one of the values, case-sensitively |
| `startsWith(...)`, `endsWith(...)` | starts (ends) with the text |
| `minLength(n)`, `maxLength(n)`, `fixedLength(n)` | at least, at most or exactly `n` characters |
| `hexCode` | `#` and 3, 4, 6 or 8 hex digits |
| `iban` | validator.js's country formats and the mod-97 check |
| `jwt` | three `.`-separated parts of URL-safe base64 |
| `postalCode(SA, ...)` | validator.js's pattern of each country named |
| `creditCard`, `creditCard(visa, amex)` | validator.js's card number shapes (of the providers named) and the Luhn check |
| `coordinates` | `latitude,longitude` in decimal degrees |

`mobile` takes the locales `ar-AE`, `ar-BH`, `ar-EG`, `ar-JO`, `ar-KW`,
`ar-OM`, `ar-QA`, `ar-SA`, `de-DE`, `en-CA`, `en-GB`, `en-IN`, `en-US`,
`es-ES`, `fr-FR` and `tr-TR`, and needs at least one (VineJS's default,
every locale, isn't carried). `postalCode` takes `CA`, `DE`, `ES`, `FR`,
`GB`, `SA` and `US`. `creditCard` takes `amex`, `dinersclub`,
`discover`, `jcb`, `mastercard`, `unionpay` and `visa`. Lengths count
characters, as the schema's own length checks do. Each check but `email`,
`url` and `ipAddress` is a function generated into `dto.rs`; `regex`,
`mobile` and `postalCode` match with the `regex` crate (which `validator`
already depends on), so the data layer's `mod.rs` names `regex = "1"`.

The transforms run while the body is read, in the order listed, so the
checks and the database see the transformed value; `null` still clears a
nullable column:

- `trim`: leading and trailing whitespace removed;
- `toLowerCase`, `toUpperCase`: Unicode case mapping;
- `normalizeEmail`: validator.js's `normalizeEmail` with its defaults. The
  domain is lowercased, and so is the local part. For `gmail.com` and
  `googlemail.com`, the local part loses a `+` subaddress and its single
  dots (a run of two or more stays), and the domain becomes `gmail.com`.
  For iCloud (`icloud.com`, `me.com`) and Outlook.com's domains, the local
  part loses a `+` subaddress; for Yahoo's, a `-` subaddress (the last
  `-` and what follows). Yandex's domains become `yandex.ru`. A value with
  no `@`, or whose local part would end up empty, is kept as given.

The OpenAPI document shows what utoipa can: `format` (`email`, `uri`,
`ipv4`, `ipv6`, `uuid`), `pattern`, `minLength` and `maxLength`. Every
rule and transform is also listed in the field's description (an `in`
list included: utoipa can't put `enum` on a `String` field).

The run fails on an unknown rule (listing the valid ones), bad arguments,
a rule given twice, a rule on a column that isn't plain text (a number, an
enum, a typed id), on a managed or generated column, and on the rules left
out: `confirmed`, `sameAs` and `notSameAs` (cross-field rules belong in
hand-written request types) and `activeUrl` (it looks up DNS while
validating).

## `search`

```toml
[api]
output = "src/features"

[api.search]
projects = ["name"]
tasks    = ["title"]
```

The text columns a list's `q` matches. `GET /projects?q=launch` answers the
rows where any of them contains `launch`, ignoring case (`ILIKE`); `%`, `_`
and `\` in `q` match themselves, and an empty `q` matches every row. A
table without search columns takes no `q`: its list query is `PageQuery`
instead of `ListQuery`, and the document lists no `q` parameter.

<!-- source: docs/examples/api/src/features/projects/dto.rs -->
```rust
/// `GET /api/v1/projects`'s query string: `?page=&per_page=&q=&sort=&order=`.
/// `q` matches `name`.
/// `sort` takes a [`ProjectSort`], `created_at` unless given.
pub type ListProjectsQuery = ListQuery<ProjectSort>;
```

## `sort`

```toml
[api]
output = "src/features"

[api.sort]
products = ["name", "price", "created_at"]
```

The columns a list's `sort` takes: numbers, dates, times or text. They
become the table's sort key enum in the data layer, `ProductSort`, which is
also a schema in the document; `?sort=` with any other value is a
`400 bad_request`. A list sorts by `created_at` when it is a sort key and
the query names none, otherwise by the first key; `order` is `desc` unless
given, and ties are broken by the primary key.

Without an entry, the sort keys are the key columns and `created_at` (or
the first sortable column when there is no `created_at`). The example sets
none, so its projects sort by `id` or `created_at`:

<!-- source: docs/examples/api/src/db/projects/listing.rs -->
```rust
/// The columns `GET /api/v1/projects?sort=` takes, in `snake_case`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ProjectSort {
    /// Sorts by the `id` column.
    Id,
    /// Sorts by the `created_at` column.
    CreatedAt,
}
```

## `skip`

```toml
[api]
output = "src/features"

[api.skip]
audit_log = ["create", "update", "remove"]
users     = ["lookups", "relationships"]
```

Endpoints to leave out of a table's feature. The route, the handler, its
service methods and its path in the document all go:

| Name | Route |
|---|---|
| `list` | `GET /` |
| `get_one` | `GET /{id}` |
| `create` | `POST /` |
| `update` | `PATCH /{id}` |
| `remove` | `DELETE /{id}` |
| `search` | `POST /search` (with `search_endpoint`) |
| `restore` | `POST /{id}/restore` (soft-delete tables) |
| `lookups` | every `GET /by-<column>/{value}` |
| `relationships` | every relationship route: `?include=`, `GET /{id}/<name>`, and the many-to-many writes |

Some endpoints don't exist to begin with: a table with no primary key has
no keyed routes, one with nothing to insert no `create`, and one with no
column to change no `update`.

## `search_endpoint`

```toml
[api]
output = "src/features"
search_endpoint = true
```

Adds `POST /search` to every feature, off by default. Its body filters by
any column, with the data layer's conditions, and sorts and pages like the
list:

```jsonc
{
  "filter": {"name": [{"contains": "launch"}], "open_tasks": [{"gt": 0}]},
  "sort": "created_at",
  "order": "asc",
  "page": 1,
  "per_page": 20
}
```

Every condition must hold. `sort` must be one of the list's sort keys, or
the answer is a `422` with the details under `sort`. A hidden column can't
be filtered on (`422`, details under `filter.<column>`), and relationship
filters are left out. The answer is the list's page.

## `openapi`

```toml
[api]
output = "src/features"
openapi = false
```

On by default: every generated handler carries `#[utoipa::path]` and every
body derives `ToSchema`. `false` turns the document off, and the generated
code then mentions no utoipa. The files `init` scaffolded still serve the
document, so remove its lines from them by hand:
[Turning the document off](openapi.md#turning-the-document-off).

## Conventions that shape the API

The `[conventions]` section says what a column means in every table that
has it ([README](../../README.md#conventions-and-the-rok-preset)). Four of
them change the API:

```toml
[conventions]
managed_columns    = ["updated_at"]
soft_delete_column = "deleted_at"
version_column     = "row_version"
tenant_column      = "organization_id"
```

- **Managed columns** are left out of both bodies, so a caller can't set
  them, and stay in the response. The database maintains them, with a
  default or a trigger.
- **Soft delete:** `DELETE /{id}` sets the column instead of deleting the
  row, and `DELETE /{id}?permanent=true` deletes it. `POST /{id}/restore`
  clears it again and answers the row (`404` for a row that isn't
  soft-deleted). Reads and lists skip soft-deleted rows. The column is left
  out of both bodies.
- **Version column:** every answer holding one row carries an `ETag` of
  its version, and `PATCH` needs it back in `If-Match`. Without `If-Match`
  the update is a `428 precondition_required`, and with a stale one a
  `412 version_conflict` ([the wire standard](wire-standard.md#versioned-rows)).
- **Tenant column:** every route of the table is scoped to the request's
  tenant, and the column is left out of the bodies. Handlers take a
  `Tenant<T>` extractor (`shared/tenant.rs`), where `T` is the tenant
  column's Rust type. It reads the tenant from the request's extensions, so
  a middleware of yours, mounted in `app.rs`, must put it there
  (`req.extensions_mut().insert(tenant)`) after deciding it from the
  caller. A request reaching a tenant-scoped handler without one is a
  `500 internal_error`, logged as a wiring bug.

### Keys the server makes: `primary_key_ids`

```toml
[conventions]
primary_key_ids = "uuid_v7"
```

With `primary_key_ids`, the server makes each new row's key, so the create
body has no key field. `init` sets `uuid_v7`: UUIDs ordered by creation
time. The values are `uuid_v4` and `uuid_v7` (`UUID` keys), `ulid` (`UUID`
or text keys) and `cuid2` (text keys only); the
[README](../../README.md#conventions-and-the-rok-preset) has what each makes
and the dependency it needs. `none` turns it off.

Without it, a key column with a database default is optional in the create
body (left out, the database makes it), and one without a default is
required: the caller supplies the key. An identity or serial key is never
in the body.

## Checks from the schema: `unique` and `exists`

Two checks need no key: the schema decides them. Before a create or update
writes, its `create_in` or `update_in` checks, on the write's transaction,
that every foreign key in the body names a row the caller can see
(`exists`) and that no other row already holds a unique key's values
(`unique`), and answers every failure in one `422 validation_failed`
([the wire standard](wire-standard.md#422-validation) has the details). A unique
index on `lower(<column>)` is checked ignoring case, and a partial one on
`<soft-delete column> IS NULL` counts live rows only. The data layer's
`db/<table>/listing.rs` holds each key's read, `<columns>_taken`.

The database's constraints stay, so a race the checks miss still answers
`409 conflict`. To drop or change a check, override `create_in` or
`update_in` in the feature's `service.rs`: the override replaces the
generated default, checks included, and can call the same `repo::` reads
and `FieldFailures` (`shared/validation.rs`).
