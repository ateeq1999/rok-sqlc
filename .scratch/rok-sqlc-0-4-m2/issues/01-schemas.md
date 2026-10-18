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

**Status:** ready-for-agent

- [ ] Generator tests cover the derives and value-type overrides, and that output is unchanged without OpenAPI.
- [ ] `tests/api-app` compiles with every type deriving its schema, and passes the lint gate and `check-arch`.
