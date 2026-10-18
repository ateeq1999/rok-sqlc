# 08: Lookups, relationships, soft delete, concurrency, tenants, search

**What to build:** The remaining routes in ADR 0016, each skippable with `api.skip`:

- **Unique lookups:** `GET /by-<column>/{value}` for each single-column unique key that isn't hidden.
- **Relationships:**
  - `GET /{key}?include=<name>` for to-one relationships (joined read, embedded);
  - `GET /{key}/<name>` for has-many (loader for one row);
  - many-to-many: `GET /{key}/<name>`, `PUT /{key}/<name>` (sync, with an id list), and `PUT`/`DELETE /{key}/<name>/{other}` (attach and detach).

  `[relationships]` renames and opt-outs apply. Related rows go through their own features' `XResponse` (hidden columns dropped). Ownership and access of the related table apply too: a caller can't read another owner's rows through a relationship.
- **Soft delete:** `DELETE` soft-deletes, `POST /{key}/restore`, `DELETE /{key}?permanent=true`.
- **Versioned tables:** reads send `ETag: "<version>"`. `PATCH` requires `If-Match`: a missing header gives 428 `precondition_required`, a stale one 412 `version_conflict`.
- **Tenant-scoped tables:** handlers take the `Tenant` extractor from ticket 05.
- **`POST /search`:** only when `api.search_endpoint = true`. It takes `{filter, sort, page}` as JSON and answers like `list`. A filter or sort on a hidden column is a 422 `validation_failed` naming the field.

Every new route follows the service trait pattern (with `_in` variants for writes) and the wire standard.

**Blocked by:** 07

**Status:** resolved

- [x] The `tests/api-app` fixture covers every route and its failure cases against PostgreSQL, including ownership through relationships.
- [x] The lint gate passes.

## Comments

Resolved on branch `rok-sqlc-0-4-08`.

**Generator.** `src/generator/api/`: `feature.rs` (new `Endpoint::{Search, Restore}`, `Lookup`, and `Feature` fields `tenant`, `version`, `soft_delete`, `lookups`, `relations`, `filter_fallible`; the tenant/version refusal is gone), `relations.rs` (the `Relation` model, built in `api::features` once every feature exists), `relation_routes.rs` (routes, handlers, `dto.rs` items), `relation_service.rs` (service methods and the `listing.rs` join-table writes), `search.rs` (body, filter, service method, `listing.rs` reads), `scope.rs` (tenant and caller parameters). `layout.rs` gained `Uses` (handlers, service and dto imports are collected while rendering) and `push_call`/`impl_head`, which follow rustfmt's call, chain and header rules (checked by the fixture and by `features_with_long_names_are_rustfmt_clean`, now with long-named tenant, version, soft-delete, owned and many-to-many tables).

**What ticket 10 should describe.**
- **Lookups:** `GET /by-<column>/{value}` (kebab-case column) for each single-column data layer `get_by_<col>` whose column isn't hidden; owner and tenant apply; service `get_by_<col>(value)`. `[api.skip] lookups` drops them.
- **Search** (`[api] search = true`): `POST /search` with `{filter, sort, order, page, per_page}` (deviation: `order` and `per_page` added to the ticket's `{filter, sort, page}`, like the list query), answering a `PaginatedResponse`. `filter` is `XSearchFilter`: the data layer's `XFilter` fields less relationship filters and `include_deleted`, `deny_unknown_fields`. A hidden column's field (and its `_is_null`) exists only to be rejected: `422 validation_failed`, details under `filter.<column>` (code `hidden`). `sort` is a string checked against the list's `XSort` (`XSort::from_name`, details under `sort`, code `sort_key`). Data layer: `search_matching`/`count_search` in `listing.rs`, scoped by tenant and owner.
- **Soft delete:** `DELETE /{key}` soft-deletes, `?permanent=true` (`RemoveXQuery`) calls `remove_permanently(_in)`; `POST /{key}/restore` → `restore(_in)`, `200` with the row (`404` if it isn't soft-deleted). An owned table's `lock_owned` now counts soft-deleted rows (restore checks the owner first).
- **Versions:** every row answer of a versioned table carries `ETag: "<version>"` (`shared::response::etag`); `PATCH` takes `IfMatch<T>` (`shared::validation`): missing `428 precondition_required`, unparsable or stale `412 version_conflict`; `update(_in)` takes `version` after the key and maps `UpdateOutcome`.
- **Tenants:** handlers take `Tenant<T>` (T = the tenant column's type) after the caller extractor, so a caller without credentials gets `401` before a missing tenant's `500`; service methods take `tenant` first, then `user`.
- **Relationships** (to tables that have a feature; `[relationships]` renames and opt-outs apply; `[api.skip] relationships` drops them): to-one → `GET /{key}?include=<name>` (one at a time; `GetXQuery`/`XInclude`; answers `XWith<Name>Response` = the row flattened plus `<name>`: the related response or `null`), via the data layer's `get_with_<name>`; to-many → `GET /{key}/<name-kebab>` answering `{"items": [...]}` (`shared::response::ItemsResponse`), via `load_<name>`; many-to-many also `PUT /{key}/<name>/{other}` (attach) and `DELETE` (detach, `404` when not paired), `204`; an attach of a missing row is the FK's `409`. `PUT /{key}/<name>` (`{"ids": [...]}`, `shared::validation::IdList`) syncs only from the join table's first key column's side (where the data layer has `sync`) and only when the related table isn't owned (a sync would drop other owners' pairs) — deviation. Writes need single, `Copy` keys on both sides. Routes take the stricter of the two features' access; an `?include=` needing a stricter caller or a tenant the feature lacks extracts it from the request in that branch. Related rows of an owned table are filtered to the caller (embedded `null`, left out of `items`, `404` on attach/detach). A relation whose calls need tenants of different types is not served.
- **Architecture:** a feature names another's response only as `super::super::XResponse`, re-exported by `generated.rs`, so check-arch rule 4 holds; many-to-many writes go through wrappers in the feature's own `db/<table>/listing.rs` (`attach_<name>`, `detach_<name>`, `sync_<name>`), so the cross-feature write rule holds; `repo.rs` re-exports `relationships::*` when the feature has relations. `listing.rs` is generated for list, search, owned features (`get_owned`, `lock_owned`, `owned_by`, `owned`, `retain_owned`) and many-to-many writes.

**Fixture.** `tests/api-app/migrations/0003_extended.sql`: `documents` (tenant, `revision` + trigger, `deleted_at`, admin access *and* owner — closes 07's gap — hidden unique `secret`, unique `(tenant_id, code)`, belongs to `labels` and uniquely to `tasks`) and `task_labels` (owned tasks ↔ public labels). The harness turns on the three conventions and `search_endpoint`, renames `notes.members` to `memberships`, and adds `documents`' `[api]` entries for the second run only. `tests/common` gained `With`/`call_with` (tenant via a `wrap_fn` middleware, `If-Match`) and `Answer::etag`. New tests: `documents.rs`, `lookups.rs`, `relationships.rs`; `tests/api_features.rs` replaces the refusal test with `extended_routes_follow_the_schema` and `skip_and_relationship_opt_outs_drop_the_extended_routes`.
