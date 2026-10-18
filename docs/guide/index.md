# The rok-sqlc API guide

How to build an actix-web API with rok-sqlc: `rok-sqlc init` scaffolds the
application, `rok-sqlc g` generates the data layer and one API feature per
table, and `rok-sqlc check-arch` keeps the architecture. The
[README](../../README.md#generated-apis) documents the commands and the
`[api]` configuration. The decisions behind them are in ADR 0016
(`docs/adr/0016-api-features-for-actix-web.md`).

## Pages

- [Architecture guidelines](architecture.md): the layout, the six rules
  `check-arch` enforces, services, transactions and ports.
- [Generated and scaffolded files](layout.md): which files rok-sqlc owns,
  which are yours, and what regeneration does to each.

## Coming

These pages arrive with the `templates/actix-api` starter project:

- **Getting started:** from `rok-sqlc init` to a running API.
- **Configuring API features:** access, owners, hidden columns, search,
  sort and skipped endpoints, worked through on one schema.
- **The wire standard:** every status, error code and body.
- **OpenAPI:** the generated document and Swagger UI.
- **Testing an API:** service tests with `#[sqlx::test]` and API tests with
  `actix_web::test`.
- **Deploying:** configuration, health checks and migrations.
