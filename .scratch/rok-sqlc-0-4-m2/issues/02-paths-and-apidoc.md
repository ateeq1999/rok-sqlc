# 02: Path annotations, `ApiDoc`, and serving the document

**What to build:**

**Path annotations.** Every generated handler gets `#[utoipa::path]`:
- `method`, and an absolute `path` that includes `api.prefix` and the route base, with `{param}` path parameters typed and described;
- `tag` (the route base without the leading slash), and `operation_id` set to the handler name;
- `request_body`, and `params(...)` (path parameters plus the query `IntoParams`);
- `responses`: exactly the statuses the handler can answer (see the spec's response list), each error status with body `ErrorBody`;
- `security(("bearer_auth" = []))` when the route needs a caller.

Versioned routes document `ETag` and `If-Match`. A tenant route's tenant comes from middleware, so the operation doesn't show it. Health routes get annotations too, under a `health` tag.

**`ApiDoc`.** `api.output/generated.rs` gains `pub struct GeneratedApiDoc`, a `#[derive(OpenApi)]` with every generated path and schema, the tags, and a `BearerAuth` `Modify` addon defining `bearer_auth` (HTTP bearer, format JWT).

**Scaffolded files** (written once, never overwritten):
- a scaffolded `src/openapi.rs`, for your hand-written paths and schemas: `pub struct ApiDoc` plus `pub fn document() -> utoipa::openapi::OpenApi`, which merges `GeneratedApiDoc` into the user's `ApiDoc` and sets the title and version from the crate;
- the `init` templates for `app.rs` and `lib.rs`, which mount `utoipa_swagger_ui::SwaggerUi::new("/docs/{_:.*}").url("/api-docs/openapi.json", document())`.

  Check how utoipa-swagger-ui 9's actix integration expects the route pattern, and test that `/docs/` serves the UI and `/api-docs/openapi.json` the document. Existing projects' scaffolded files don't change, so the guide says how to add it by hand.

**Check-arch.** `check-arch` must accept the new files and imports: `utoipa` is allowed wherever the six rules allow it, and `openapi.rs` is a top-level file. Update `check_arch/rules.rs` and the rule texts if needed.

**Dependency hints:** `utoipa-swagger-ui = { version = "9", features = ["actix-web"] }`, in `rok-sqlc.toml`'s comment block from `init` and in the generated `mod.rs` hints.

**Blocked by:** 01

**Status:** resolved

- [x] `tests/api-app` serves `/api-docs/openapi.json` and `/docs/`, and an actix test checks that a sample path's annotations (statuses, security, params) match the handler's behaviour.
- [x] A fresh `init` + `g` project builds clippy-clean, passes `check-arch`, and serves both routes.

## Comments

Resolved on branch `rok-sqlc-0-4-m2-02`.

What was built:

- `src/generator/api/paths.rs` renders each handler's `#[utoipa::path]` (gated on `Feature::openapi`): method, absolute path, `tag` (`Feature::tag`, the route base without its slash, new on `Feature`), `operation_id` = handler name, `params(...)` (typed key params as their inner type, the query `IntoParams` types, `If-Match` as a header on a versioned update), `request_body`, responses, and `security(("bearer_auth" = []))`. Hooks: `handlers.rs` (endpoints, lookups) and `relation_routes.rs` (list/sync/attach/detach).
- Statuses per handler: `400` when a path param can fail to parse (not text), any query, or a JSON body; JSON bodies also `413` and `422` (serde's "unfit" body is a 422, `BodyRejection::unfit`); `404` on keyed routes; `409` on create (PK/unique/FK), update (unique/FK), remove (table referenced: a non-belongs-to relationship), restore (unique keys), sync/attach; `412`/`428` on a versioned update; `401` (user) or `401`/`403` (admin) from access. `GET /{id}` with `?include=` adds the stricter relationships' access statuses, and a public feature with a stricter include gets `security((), ("bearer_auth" = []))` (optional). `500` is never listed. Error descriptions are short so each response fits a line; responses with headers (`ETag`, `Location`) are split over lines. rustfmt leaves these attributes alone.
- `src/generator/api/api_doc.rs`: `GeneratedApiDoc` (`#[derive(utoipa::OpenApi)]`) and `pub struct BearerAuth` (`Modify`, HTTP bearer, format JWT) at the end of `generated.rs` when `api.openapi`; lists health's paths and every handler, registers `SortOrder`, each `XSort`, each `XInclude` and each `XWithYResponse` (the embedding responses are not referenced from the `get_one` path, whose 200 body is `XResponse`), and tags `health` plus one per feature.
- Health: `shared/health.rs` handlers are annotated (operation ids `live`/`ready`, not the target's `liveness`/`readiness`, so operation ids stay handler names), `ready` documents `503` with `ErrorBody`.
- `init`: new scaffolded `src/openapi.rs` (`ApiDoc` with empty `paths()`/`components(schemas())`, `document()` merging `GeneratedApiDoc` and setting title/version from `CARGO_PKG_*`), `lib.rs` declares `pub mod openapi;`, `app.rs`'s `configure` mounts `web::redirect("/docs", "/docs/").see_other()` and `SwaggerUi::new("/docs/{_:.*}").url("/api-docs/openapi.json", document())` (utoipa-swagger-ui 9.0.2's actix integration registers `Resource::new(path)`, so `/docs` alone needs the redirect, as the target's 303). Hint `utoipa-swagger-ui = { version = "9", features = ["actix-web"] }` in `init`'s `rok-sqlc.toml` and `db/mod.rs` (`openapi::SWAGGER_UI_HINT`).
- `check-arch` needed no change: no rule restricts `utoipa`, and top-level files aren't restricted. `api_app_passes_check_arch` and the init e2e `check-arch` pass.
- Tests: `tests/api_paths.rs` (generator output), `src/init/tests.rs`, `tests/cli/init.rs` (skeleton list), `tests/api-app/tests/docs.rs` (served document and UI, `/docs` redirect, every `$ref` resolves, `GET /tasks/{id}` statuses/security/params against the handler, and every operation called without a token answers `401` when it documents `bearer_auth`, else a documented status), and the init e2e now writes and runs a `tests/docs.rs` serving both routes. `tests/api-app` gains `utoipa-swagger-ui` (lock: additions only); `.gitignore` adds its scaffolded `src/openapi.rs`. `docs/examples/api` regenerated (its scaffolded `app.rs`/`lib.rs` don't serve the docs yet), and the architecture guide's handler sample shows the annotation.

What ticket 03 must know:

- utoipa-swagger-ui's build script downloads the Swagger UI zip with `curl` (no `vendored` feature); CI needs network, or set `SWAGGER_UI_DOWNLOAD_URL=file://...`.
- The operations carry `summary`/`description` from the handlers' doc comments (utoipa does this); the snapshot will include them.
- `docs/examples/api` must add `pub mod openapi;`, `src/openapi.rs`, the `app.rs` mounting and the dependency by hand to serve the docs (existing projects don't get scaffolded files); the README/guide should say the same.
- The structural check "operation ids are handler names" holds for health too (`live`, `ready`); operation ids aren't unique across tags (`list` in every feature), as in the target.
