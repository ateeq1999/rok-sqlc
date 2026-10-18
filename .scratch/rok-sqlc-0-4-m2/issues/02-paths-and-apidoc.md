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

**Status:** ready-for-agent

- [ ] `tests/api-app` serves `/api-docs/openapi.json` and `/docs/`, and an actix test checks that a sample path's annotations (statuses, security, params) match the handler's behaviour.
- [ ] A fresh `init` + `g` project builds clippy-clean, passes `check-arch`, and serves both routes.
