# 05: Immutable columns (`[api.immutable]`)

**What to build:** An **immutable column** (GLOSSARY) is set on create and never changed through the generated update. Decided with the maintainer on 2026-10-19 (Q50 and the follow-up).

**Config.** `[api.immutable] users = ["email"]` takes a list of columns per table. The JSON payload uses the same shape. It's part of `ApiConfig`, layered like `hidden`, documented in the README `config-reference` (drift tests). These fail the config check with a clear message: an unknown table or column, a key column (keys are already never updated), an owner or tenant column (already immutable), and a managed column (never written at all).

**Generation:**
- `CreateXDto` keeps the column (required if `NOT NULL` without a default).
- `UpdateXDto` leaves it out.
- A `PATCH` body naming an immutable column answers `422 validation_failed` with details under the field (code `immutable`, message "`<column>` can't be changed"). It is never silently ignored: the update body must reject that field explicitly, even though unknown fields are otherwise ignored.
- Responses are unchanged.
- The OpenAPI document shows the column absent from `UpdateXDto`.
- The data layer's `UpdateX` patch keeps the column, so a hand-written service (for example, a verified email change) can still write it.

**Template and contract.** In `templates/actix-api/rok-sqlc.toml`, add `[api.immutable] users = ["email"]`. Email changes go through milestone 5's verification flow. Remove KNOWN entry 2 (`UpdateUserDto` has `email`) from `templates/actix-api/tests/contract.rs`. The contract then matches the target there. Regenerate the template's checked-in files (`ROK_SQLC_UPDATE_EXPECTED=1`).

**Docs:** `docs/guide/configuring.md` (the key, with examples: slug, order number, external id, currency), `wire-standard.md` (the `immutable` detail code), README, CHANGELOG `[Unreleased]`, and ADR 0016's list of `[api]` keys.

**Blocked by:** 01, 02, 03 (all done)

**Status:** resolved

- [x] Config tests (each check) and generator tests (create keeps it, update drops it, OpenAPI).
- [x] `tests/api-app` gets an immutable column, and a PATCH naming it is a 422 against PostgreSQL. The template's contract passes with KNOWN entry 2 removed.

## Comments

Resolved on branch `rok-sqlc-0-4-m3-05`.

- **Config:** `ApiConfig::immutable` (`BTreeMap<String, Vec<String>>`), in `ApiKeys` (file and JSON payload), merged by table like `hidden`. `api_config::check` now takes the `Conventions` and rejects an unknown table or column, a key, owner or tenant column ("which no update changes anyway"), and a managed or generated column ("which generated code never writes"). Generated columns are a small addition to the ticket's list.
- **Generation:** `Feature::immutable` / `is_immutable`. `UpdateXDto` drops the column. In its place is `Option<serde_json::Value>` with `#[schema(ignore)]` (when `openapi`), `#[serde(deserialize_with = "present")]` (so `null` counts too) and `#[validate(custom(function = "immutable", message = "`<col>` can't be changed"))]`, which gives code `immutable`. `From<UpdateXDto>` sets it to `None`, and `UpdateX` keeps it. A table whose only patchable columns are immutable or the owner gets no `PATCH`. `validate::push_rule` is now `pub` for the attribute layout.
- **Fixture:** `accounts.email` is immutable in `tests/api-app` (`access.rs::an_immutable_column_is_set_on_create_and_never_patched`). The expected OpenAPI document was regenerated.
- **Template:** `[api.immutable] users = ["email"]`, KNOWN entry 2 removed, `users/dto.rs` regenerated, README updated.
- **Docs:** configuring.md (`immutable` section), wire-standard.md, troubleshooting.md (errors and the unknown-field list), README reference and prose, CHANGELOG, ADR 0016 bullet.
