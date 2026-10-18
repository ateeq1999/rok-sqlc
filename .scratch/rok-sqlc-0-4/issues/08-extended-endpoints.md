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

**Status:** ready-for-agent

- [ ] The `tests/api-app` fixture covers every route and its failure cases against PostgreSQL, including ownership through relationships.
- [ ] The lint gate passes.
