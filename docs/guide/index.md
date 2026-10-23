# The rok-sqlc API guide

How to build an actix-web or axum API with rok-sqlc: `rok-sqlc init`
scaffolds the application, `rok-sqlc g` generates the data layer and one
API feature per table, and `rok-sqlc check-arch` keeps the architecture.
The [README](../../README.md#generated-apis) documents the commands and
the `[api]` configuration. The decisions behind them are in ADR 0016
(`docs/adr/0016-api-features-for-actix-web.md`), those behind validation
in ADR 0017 (`docs/adr/0017-validation-rules.md`), those behind the
templates' accounts in ADR 0018
(`docs/adr/0018-auth-as-template-features.md`), and those behind axum in
ADR 0019 (`docs/adr/0019-axum-support.md`).

A project uses one framework, picked with `[api] framework`
([Configuring](configuring.md#framework)); actix-web is the default. The
pages show actix-web code; where axum differs, a paragraph or a sample
says how. Everything else (the services, the data layer, the wire
answers, the OpenAPI document) is the same for both.

The guide's code samples come from projects that compile, mostly
[`docs/examples/api`](../examples/api), and a test keeps the two in sync.

## Pages

Start here:

- [Getting started](getting-started.md): from `rok-sqlc init` (or
  `init --framework axum`, or a template: `init --template actix` or
  `axum`) to a running API, its first requests and Swagger UI.
- [Configuring API features](configuring.md): every `[api]` key (access,
  owners, hidden columns, search, sort, skipped endpoints, routes) and the
  conventions and key generation that shape the API.

How the code is organized:

- [Architecture guidelines](architecture.md): the layout, the six rules
  `check-arch` enforces, services, transactions and ports.
- [Generated and scaffolded files](layout.md): which files rok-sqlc owns,
  which are yours, and what regeneration does to each.
- [Customizing the generated API](customizing.md): service overrides,
  state, ports, and hand-written features.
- [Accounts and sign-in](auth.md): the templates' sign-up,
  sign-in, password and email flows, two-factor sign-in, API keys,
  sessions, mail, and adding a flow.
- [Files and storage](files.md): the templates' uploads, profile photos,
  the storage port, S3 and the limits.

What the API answers:

- [The wire standard](wire-standard.md): every status, error code and
  body, the list shape, and `Location`, `ETag` and `If-Match`.
- [The OpenAPI document](openapi.md): what is generated, your hand-written
  paths, and serving the document and Swagger UI.

Running it:

- [Testing an API](testing.md): service tests and API tests with
  `actix_web::test` (or axum's router and `tower::ServiceExt::oneshot`),
  against PostgreSQL.
- [Deploying](deploying.md): configuration, the pool, health checks,
  migrations, logs, and the checks to run in CI.
- [After a schema change](after-a-schema-change.md): what regenerates, what
  stays yours, what breaks, and `--check` in CI.
- [Troubleshooting](troubleshooting.md): each configuration, generation,
  `check-arch` and start-up error, with its fix.
