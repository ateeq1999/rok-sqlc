# The two starter templates (research, 2026-10-18)

Sources: `D:\DEV\open-source\Project-Starters\actix-api-starter` and `D:\DEV\open-source\Project-Starters\axum-api-starter-v2`. The target OpenAPI document (`openapi.json` here) is the **axum** starter's, so the axum starter defines the wire standard (ADR 0016).

## Shared by both starters, so generated code can always do the same

- `ValidatedJson<T>` over `validator` derives (validator 0.20 in actix, 0.21 in axum). Request DTOs derive `Debug, Deserialize, Validate, ToSchema`. Optional fields use `#[serde(default)]`.
- `AuthUser {id, role}` and `AdminUser(AuthUser)` extractors. `Role` is `User`/`Admin`, snake_case, stored in a TEXT column with a CHECK.
- utoipa 5, with a `bearer_auth` HTTP bearer (JWT) scheme added by a `Modify` addon. A hand-listed `#[derive(OpenApi)] ApiDoc` serves the spec at `/api-docs/openapi.json`.
- sqlx 0.9 runtime `query_as::<_, Row>(literal).bind()`, never `query!`. Column lists stay literal `&'static str` (axum uses `macro_rules! columns` + `concat!`) because of sqlx 0.9 `SqlSafeStr`.
- PATCH with `COALESCE($n, col)` and `RETURNING`. UUID v4 ids generated in Rust. Entity and DTO separate, joined by `From`.
- Paging defaults: page 1, per_page 20, max 100.

## Axum starter (the standard)

- **Layout:** single crate. `src/{main,lib,app,state}.rs`, `common/{error.rs, dto/{pagination,message_response}.rs, extractors/{validated_json,validated_query}.rs, middleware/, security/}`, `infra/{database,openapi,telemetry,jobs/}`, `modules/<name>/{mod, controller(s), service(s), repository, entity, error, dto/}`. Docs in `docs/{architecture,adding-a-feature,endpoints,recipes,testing}.md`.
- **Errors:** `AppError {BadRequest, Unauthorized, Forbidden, NotFound, Conflict, TooManyRequests, PayloadTooLarge, UnsupportedMediaType, Internal(anyhow), Database(sqlx), Validation(ValidationErrors)}`. The body is `{"error":{"code","message","details"?}}`.

  | Variant | Status | Code |
  |---|---|---|
  | BadRequest | 400 | `bad_request` |
  | Unauthorized | 401 | `unauthorized` |
  | Forbidden | 403 | `forbidden` |
  | NotFound | 404 | `not_found` |
  | Conflict | 409 | `conflict` |
  | TooManyRequests | 429 | `too_many_requests` |
  | PayloadTooLarge | 413 | `payload_too_large` |
  | UnsupportedMediaType | 415 | `unsupported_media_type` |
  | Validation | 422 | `validation_failed` |
  | Database / Internal | 500 | `internal_error`, message "Something went wrong", logged |

  Each feature has its own error enum with `From<XError> for AppError`. A unique violation (`e.is_unique_violation()`) maps to the feature's conflict error.
- **Success responses:** bare `Json<XResponse>`, `(StatusCode::CREATED, Json<..>)` for create, `StatusCode::NO_CONTENT` for delete, and `MessageResponse {message}` for actions.
- **Lists:** a per-feature `ListXQuery {page, per_page, q (max 100), sort, order}` with `IntoParams` and `.pagination()`. The response is `PaginatedResponse<T> {items, page, per_page, total: u64}`. `q` is ILIKE with `%`/`_` escaped, using the `$2::text IS NULL OR …` pattern. `UserSort {created_at, email}`, `SortOrder {asc, desc}` (default desc).
- **Extractors:** `ValidatedJson` maps a `JsonRejection` to `BadRequest(rejection.body_text())`, and `.validate()?` to 422. `ValidatedQuery` does the same for queries.
- **Routing:** `Router::new().merge(health::router()).nest("/api/v1", modules::router(&state))`. Health is `/health/live` (`{status:"ok"}`) and `/health/ready` (`SELECT 1`, 200 or 503). Swagger UI is at `/docs`. Per feature: `.route("/", get(list).post(create)).route("/{id}", get(get_one).patch(update).delete(remove))`. Paths are plural kebab-case, tags lowercase kebab-case, and `#[utoipa::path(path="/api/v1/...")]` takes absolute paths.
- **State and auth:** `AppState` derives `Clone, FromRef`, with fields `Arc<XService>`. Handlers take `State<Arc<XService>>`. `AuthUser` accepts a JWT or an API key, verified against the DB; the guard is the extractor in the handler signature. Ownership is enforced in SQL (`WHERE id=$1 AND owner_id=$2`), so another caller's row is a 404.
- **Database:** `PgPoolOptions` (acquire 5 s), then `sqlx::migrate!("./migrations")`. Inserts bind `created_at`/`updated_at` from `Utc::now()`. A `jobs` table is claimed with `FOR UPDATE SKIP LOCKED`, and an `outbound_mail` outbox exists.
- **Tests:** `tests/<feature>.rs` with `mod common; spawn()`, a throwaway DB per test, and `oneshot`. `tests/openapi_contract.rs` checks against `tests/snapshots/openapi.json`.
- **Request id:** `x-request-id` header (tower-http), never in the body.

## Actix starter (its layout is not followed; ADR 0016 picks the guidelines' single crate)

- **Layout:** a workspace with `crates/{auth, config, db (pool+health+migrations), envelope (ApiResponse), error, macros (#[module]), mail, pagination (offset/cursor/keyset tagged meta), router (openapi, health, not_found), session (jwt, require_auth middleware, AuthUser/AdminUser), storage, users, validation}`.
- **Features:** `x_module.rs` (`#[module(prefix, controllers, protected)]`), `*_action.rs` (one handler each, actix `#[get("")]` plus `#[utoipa::path]`), `x_entity.rs`, `x_repository.rs` (async_trait port plus Pg adapter), `x_service.rs`, `dto/`.
- **Responses:** success is `{success, data, meta}`; errors are `{success:false, error:{message, details?}}` with no code; validation is 400. These differ from the standard.
- **Wiring:** `.app_data(web::Data::new(service))`, with handlers taking `web::Data<XService>`. Middleware: cors, TracingLogger, DefaultHeaders, `NormalizePath::trim`, Compress. Swagger UI at `/swagger-ui/{_:.*}`.
- **Lints:** clippy `all` deny, pedantic and nursery warn, and `unwrap_used`, `expect_used`, `panic`, `todo`, `print_stdout`, `dbg_macro` and `unsafe_code` deny. Generated actix code must pass these.
