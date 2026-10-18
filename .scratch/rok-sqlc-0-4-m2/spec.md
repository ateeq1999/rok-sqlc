# rok-sqlc 0.4, milestone 2: OpenAPI for generated API features

## Problem

Milestone 1 generates actix-web API features. They have no OpenAPI document, so a client can't discover them, and milestone 3's template can't be checked against the target spec (`../rok-sqlc-0-4/notes/openapi.json`, the axum starter's document).

## Decisions

These were settled in the 2026-10-18 design session; ADR 0016 is the authority:
- **Annotations:** every generated handler gets `#[utoipa::path]`, and every DTO and response type derives `ToSchema` (or `IntoParams` for query strings).
- **`ApiDoc`:** a generated `ApiDoc` lists every generated path and schema, with a `bearer_auth` HTTP bearer (JWT) security scheme added by a `Modify` addon. It merges with a scaffolded `ApiDoc` for the user's hand-written features.
- **Serving:** the document is served at `/api-docs/openapi.json`, and Swagger UI at `/docs`.
- **Tags:** one per feature, lowercase kebab-case (the route base without the slash).
- **Version:** utoipa 5 and utoipa-swagger-ui 9 (`actix-web` feature), the versions both starters and the target spec's producer use. The current releases are 6 and 10; moving up is a later, separate decision.

The document follows the wire standard (`../rok-sqlc-0-4/notes/openapi-standard.md`):
- **Error responses:** use one `ErrorBody` schema.
- **Responses per route:** list `200` with `PaginatedResponse<XResponse>`, create `201`, delete `204`, plus `400`/`401`/`403`/`404`/`409`/`412`/`422`/`428` exactly where the generated handler can answer them.
- **Operation ids:** the handler names (`list`, `create`, `get_one`, `update`, `remove`, …), as in the spec.
- **Security:** a route that needs `AuthUser` or `AdminUser` carries `security(("bearer_auth" = []))`, and a public route carries none.

## Scope

- Milestone 3 does the contract test against the target spec, inside the template. Milestone 2 ships a snapshot test of `tests/api-app`'s document plus structural checks.
- OpenAPI is generated whenever `api.output` is set. `[api] openapi = false` turns it off, and output is then exactly milestone 1's.

## Constraints

These are unchanged from milestone 1:
- the tool builds on Rust 1.88;
- generated code needs 1.94, passes the lint gates, `check-arch` and rustfmt, and adds no unwrap, expect or panic;
- a fresh `init` + `g` project builds clippy-clean;
- output without `[api]` is byte-for-byte unchanged.

## Tickets

`issues/01` to `issues/03`, in order: 01 → 02 → 03.
