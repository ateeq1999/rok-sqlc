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

**Status:** ready-for-agent

- [ ] Config tests (each check) and generator tests (create keeps it, update drops it, OpenAPI).
- [ ] `tests/api-app` gets an immutable column, and a PATCH naming it is a 422 against PostgreSQL. The template's contract passes with KNOWN entry 2 removed.
