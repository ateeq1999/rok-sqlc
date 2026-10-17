# 04: `db/http`: the shared envelope and the axum extractor

**What to build:** With `extractors` containing `axum`, generate `db/http/mod.rs` and `db/http/axum.rs` as ADR 0015's "Extractors are generated" paragraph describes. `db/mod.rs` declares `pub mod http;`, so `http` becomes a reserved module name, like `nullable`.

`db/http/mod.rs` holds what every framework shares:
- the error envelope as serializable types: `ErrorBody { success: false, error: ErrorPayload { message, code, details? } }`;
- the codes `MALFORMED_BODY` and `VALIDATION_FAILED`;
- the rule that maps a rejection to a status and body.

Ticket 05 reuses all of it.

`db/http/axum.rs` holds:
- `ValidatedJson<T>(pub T)` with `into_inner` and `Deref`, implementing `FromRequest<S>` for `T: DeserializeOwned + Validate`, on top of `axum::Json<T>`;
- `BodyRejection` with `Malformed { message }` and `ValidationFailure(validator::ValidationErrors)`, implementing `IntoResponse`.

The mapping from `JsonRejection`:
- bad syntax, a missing or wrong `Content-Type`, or an unreadable body is `Malformed` (400);
- a data error (missing field, wrong type, unknown label) is a 422 `VALIDATION_FAILED` carrying serde's message and no details;
- a failed `validate()` is a 422 with validator's `ValidationErrors` as `details`.

The dependency hints add `axum = "0.8"` (with `serde_json` for the body).

Test it in the fixture app with a real axum `Router` driven through `tower::ServiceExt::oneshot`, without PostgreSQL:
- a valid body reaches the handler;
- each rejection gives the right status, `code`, and body;
- a hand-written `#[derive(Deserialize, Validate)]` type works with the same extractor.

**Blocked by:** 03

**Status:** resolved

- [x] Fixture tests for every rejection path, plus a hand-written type, all on axum 0.8.
- [x] Generator tests: no `db/http` without `extractors`, and `http` is a reserved module name.

## Comments

- Implemented on branch `rok-sqlc-0-3-04`. `db/http/mod.rs` owns `BodyRejection` (status, body, `Display`), `ErrorBody`/`ErrorPayload`, the codes, `is_json`, and `validated::<T>(bytes)`, which reads and classifies JSON with serde's `Category` itself; `axum.rs` re-exports `BodyRejection`, checks `Content-Type`, reads `Bytes`, and calls `validated`. It does not go through `axum::Json<T>`, whose data-error messages carry a `serde_path_to_error` path that actix-web cannot reproduce, so both frameworks give byte-identical bodies. A data error is `ValidationFailure` holding one `__all__` error with code `deserialize` (`BodyRejection::unfit`), answered with serde's message and no `details`.
