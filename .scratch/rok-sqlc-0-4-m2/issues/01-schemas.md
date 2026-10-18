# 01: Schemas for every type the API exposes

**What to build:** With `api.output` set (and `[api] openapi` not `false`), every type a generated API exposes gets a utoipa 5 schema:
- `CreateXDto`, `UpdateXDto`, `XResponse`, the relationship response types, `XSearchFilter`, `IdList` and the `/search` body derive `ToSchema`.
- `ListXQuery` and the other query types (`GetXQuery`, `RemoveXQuery`) derive `IntoParams` with `parameter_in = Query`.
- The shared `PaginatedResponse<T>`, `ErrorBody` and `ErrorPayload` derive `ToSchema`.
- Data-layer types appear inside them: typed ids, enums, value-list enums, and `XSort`.
  - **Typed ids:** shown as their inner type through `#[schema(value_type = …)]`, e.g. a UUID id as `String` with `format = Uuid`.
  - **Enums:** use their exact database labels.
  - **Columns of foreign types:** `chrono`, `rust_decimal`, `serde_json::Value`, `PgRange`, and the network types each get the right `value_type` or format. Use utoipa's features where they cover a type, and an explicit override where they don't.
- `ErrorPayload::details` (validator's `ValidationErrors`) is a free-form object.

**Where derives go:** utoipa derives are added to `db/` types only when OpenAPI is on. Without `[api]`, or with `openapi = false`, `db/` output is unchanged.

**Dependency hints:** the generated `mod.rs` names `utoipa = { version = "5", features = [...] }`, with the features the schema actually uses (`uuid`, `chrono`, `decimal`, …).

Verify utoipa 5's derive syntax and features in the cargo registry, not from memory.

**Blocked by:** None

**Status:** resolved

- [x] Generator tests cover the derives and value-type overrides, and that output is unchanged without OpenAPI.
- [x] `tests/api-app` compiles with every type deriving its schema, and passes the lint gate and `check-arch`.

## Comments

Resolved on branch `rok-sqlc-0-4-m2-01`.

What was built:

- `ApiConfig::openapi` (default `true`) and the `[api] openapi` config key, in the README's config reference (ticket 03 still owns the docs prose and the "off is exactly milestone 1" test beyond what's here). With `openapi = false`, or without `[api]`, no generated file mentions utoipa (`tests/api_openapi.rs`).
- The switch flows as `DeriveOptions::openapi` (data layer: typed ids, enums and value-list enums in `db/types.rs`, the condition enums in `db/search.rs`, `ErrorBody`/`ErrorPayload` in `db/http/mod.rs`) and `Feature::openapi` (the feature's `dto.rs` and `XSort` in `db/<table>/listing.rs`). The shared tools' templates stay derive-free; `src/generator/api/schemas.rs::describe` adds the derives when `api.openapi` is on.
- Helpers in `src/generator/openapi.rs`: `TO_SCHEMA`, `INTO_PARAMS`, `IN_QUERY`, `push_to_schema`, `push_into_params`, `value_type`/`condition_value_type`/`push_value_type`, and `dependency_hint` (the `//   utoipa = { version = "5", features = [...] }` line in `db/mod.rs`, with `uuid`, `chrono`, `decimal` as the run's columns need; `utoipa = "5"` when none).
- `tests/api-app` gains `migrations/0004_gadgets.sql` (`gadgets`: an enum type, a value-list column, `NUMERIC`, `JSONB`, `INET`, `DATE`; public) and `tests/schemas.rs`, which checks the derived schemas' JSON. Its `Cargo.toml` adds `utoipa`, `rust_decimal`, `ipnetwork` and sqlx's `json`/`rust_decimal`/`ipnetwork` features; the lock file gained only the new packages. `docs/examples/api` is regenerated and adds `utoipa`.

How the schemas come out (utoipa 5.5.0, checked in the registry and in `tests/api-app/tests/schemas.rs`):

- Typed ids derive `ToSchema` themselves (needed inside `Equality<XId>` and `IdList<XId>`), and a DTO/response field holding one carries `#[schema(value_type = <inner sqlx type>)]` (`uuid::Uuid`, `i64`, `Option<…>`, `Option<Option<…>>`), so it shows inline as `{"type": "string", "format": "uuid"}`, as in the target document.
- Enums are `$ref`s to components named after the Rust enum (`GadgetMood`), with their exact labels via serde's renames.
- Network types are `value_type = String`. `chrono`, `rust_decimal`, UUIDs and `serde_json::Value` use utoipa's features. A search condition over a type without `ToSchema` (dates, decimals, UUIDs, network types) is `value_type = Vec<Ordered<String>>`/`Vec<Equality<String>>`: utoipa needs `T: ToSchema` there. `PgRange`, `PgInterval`, `PgMoney` and `PgTimeTz` can't reach the API (the serde check rejects them), so they get no override.
- utoipa 5 workarounds: `Ordered::Between(T, T)` carries `#[schema(value_type = Value)]` (the derive fails on a two-field variant over a generic `T`); `IntoParams` can't derive over a generic field, so `ListQuery<S: utoipa::ToSchema = String>` describes `sort` with `#[param(schema_with = sort_schema::<S>)]`, a `$ref` to `S` (text for `String`), and its impls gained the same bound; `SearchXBody::filter` carries `#[schema(default = json!({}))]` because the container's `#[serde(default)]` makes utoipa serialize each field's default and the filter isn't `Serialize`.
- `ErrorPayload::details` is `#[schema(value_type = Option<Object>)]`.
- `push_derives` (in `emit.rs`) now lays out derive lists as rustfmt does: one line up to 96 columns, one indented line up to 100 (it was 98/99, which the longer derive lists showed to be wrong).

What ticket 02 must know:

- Register the schemas `ApiDoc` needs explicitly: the `sort` parameter is a `$ref` to `XSort` that utoipa won't collect from the params, so list every feature's `XSort` and `SortOrder` (and the `XInclude` enums) in `components(schemas(...))`. Everything reached through `ToSchema` fields (enums, typed ids, `Ordered_*`/`Equality_*`, `Text`) is collected by utoipa itself.
- Query types: `ListXQuery` (alias of `ListQuery<XSort>`), `GetXQuery` and `RemoveXQuery` implement `IntoParams` with `parameter_in = Query`; use them in `params(...)`.
- Generic schemas name themselves `PaginatedResponse_NoteResponse`, `IdList_LabelId`, `Ordered_String`, etc., matching the target document's `PaginatedResponse_UserResponse`.
- The `[api] openapi` flag is `Feature::openapi` in the renderers and `api.openapi` in `shared::render`; gate the path annotations and `GeneratedApiDoc` on it. `utoipa-swagger-ui` needs adding to `dependency_hint`'s neighbour hints (and `init`'s), not to `openapi::dependency_hint`, which only names `utoipa`.
