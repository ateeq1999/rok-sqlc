# 05: The actix-web extractor

**What to build:** With `extractors` containing `actix_web`, generate `db/http/actix_web.rs` next to ticket 04's shared `db/http/mod.rs`, which it reuses.

It holds:
- `ValidatedJson<T>(pub T)` with `into_inner` and `Deref`, implementing `actix_web::FromRequest` for `T: DeserializeOwned + Validate + 'static`, on top of `web::Json<T>`;
- `BodyRejection`, the same two variants, implementing `ResponseError`, so the handler's `JsonConfig` error handler is no longer needed.

Map `JsonPayloadError`: `Deserialize` errors that are data errors (serde's `Category::Data`) give 422, and syntax errors, `ContentType` and payload errors give 400 `MALFORMED_BODY`. The status codes, codes and body match axum exactly. `extractors = ["axum", "actix_web"]` generates both, side by side.

The dependency hints add `actix-web = "4"`.

Test it in the fixture app with `actix_web::test` (`init_service` and `call_service`), without PostgreSQL. Cover the same cases as ticket 04, and assert the response bodies are identical to axum's for the same input.

**Blocked by:** 04

**Status:** ready-for-agent

- [ ] Fixture tests for every rejection path, plus a hand-written type, on actix-web 4.
- [ ] Both frameworks generate together, and bodies match across them.
