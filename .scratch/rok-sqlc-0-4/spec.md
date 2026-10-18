# rok-sqlc 0.4, milestone 1: generated API features (actix-web)

## Problem

An API built on the generated code repeats the same work for every table: routes, thin handlers, a service for business rules, list paging, access and ownership checks, and the mapping of database errors to HTTP. The maintainer wants rok-sqlc to generate all of this from `schema.sql`, under a strict feature-based architecture they can edit safely.

## Decisions

Settled with the maintainer on 2026-10-18 in a design session. `docs/adr/0016-api-features-for-actix-web.md` is the authority for every decision. Terms are in `GLOSSARY.md`: API feature, route base, service, scaffolded file, port, template. Reference material is in `notes/`:
- `architecture-guidelines-original.md`: the maintainer's guidelines, which ADR 0016 amends;
- `openapi-standard.md` and `openapi.json`: the wire standard;
- `starters.md`: how the two starter projects do things;
- `derived-schema.sql`: the schema the templates will use.

## Milestones

This spec is milestone 1. Later milestones get their own specs:
1. **This one:** the generator core for actix-web: wire standard, `init`, API features, services and ports, access, ownership, hidden columns, and `check-arch`.
2. OpenAPI generation (utoipa) and the contract test.
3. `templates/actix-api/`, the full `docs/guide/`, and the 0.4.0 release.
4. `templates/axum-api/` and the axum generator.
5. Porting the auth endpoints as hand-written features.

Milestone 1 doesn't bump the version. The CHANGELOG collects its entries under `[Unreleased]`, with a breaking section for the error-body change.

## Constraints

- The tool builds on Rust 1.88. Generated code needs 1.94 and passes the fixture lint gate (ADR 0013), and generated actix code also passes `clippy::unwrap_used`, `expect_used` and `panic`.
- A run without `[api]` and the new keys generates byte-for-byte what 0.3.0 generates. The one exception is ticket 01's documented error-body change, which applies only with `extractors`.
- The README drift tests pass: every flag and config key documented, every sample compiled.
- Scaffolded files are never overwritten and never removed as stale.
- CI runs on Linux, macOS and Windows.

## Tickets

`issues/01` to `issues/10`:
- 01, 02 and 03 can start at once.
- 04 needs 01 and 03; 05 needs 03 and 04; 06 needs 05.
- 07 needs 06; 08 needs 07; 09 needs 06.
- 10 needs 08 and 09.
