# 07: Match the behaviour agreed in ADR 0015

**What to build:** Ticket 04 and 05 implementers changed three agreed behaviours. The maintainer asked to bring them back:

1. **Build on the frameworks' JSON extractors.** `db::http::axum::ValidatedJson` should extract through `axum::Json<T>`, and `db::http::actix_web::ValidatedJson` through `web::Json<T>`, so each framework's own settings apply: axum's `DefaultBodyLimit`, and actix-web's `JsonConfig` (limit, content-type predicate). Today both read raw bytes and parse JSON themselves (`validated::<T>(bytes)` in `db/http/mod.rs`).

   Keep the cross-framework promise (same status, code and message for the same body). Classify each framework's rejection in its module, and take the message from serde's own error (the innermost `serde_json::Error`), never the framework's text: axum's data-error text carries a `serde_path_to_error` field path that actix-web lacks. Check axum 0.8's `JsonRejection` source chain and actix-web 4's `JsonPayloadError` in the registry rather than from memory.

2. **Oversized bodies answer 413.** A body over the framework's limit answers `413 Payload Too Large` with code `PAYLOAD_TOO_LARGE`, in the same envelope, on both frameworks. Today it is a 400 `MALFORMED_BODY`. Add the code next to the others in `db/http/mod.rs`. The two frameworks' default limits still differ: axum's is 2 MB, actix-web's `JsonConfig` default is 32 KB. Document this.

3. **Range rules on typed-id fields.** A field typed as a typed id over an integer or float, whose column has a value range, gets its `range` rule. Generate `impl validator::ValidateRange<T> for XId`, delegating to the inner value, as text typed ids already get `ValidateLength`. Today range rules are skipped on typed-id fields.

Decimal ranges stay a generated `custom(code = "range")` function. The orphan rule forbids implementing validator's `ValidateRange` for `rust_decimal::Decimal`, and the function already reports code `range` with `min`/`max` params. ADR 0015 should say so as a limitation, not a choice.

Update ADR 0015, the README section, and the CHANGELOG `[0.3.0]` (0.3.0 is not released yet, so no version bump).

**Blocked by:** None

**Status:** ready-for-agent

- [ ] Fixture tests for both frameworks: a configured limit gives 413 `PAYLOAD_TOO_LARGE` with identical bodies; actix's `JsonConfig` limit and axum's `DefaultBodyLimit` are honoured; every earlier rejection case still matches across frameworks.
- [ ] A typed-id field with a range rule rejects out-of-range values in the fixture's `validate()` tests.
- [ ] The full suite passes, including PostgreSQL and the README drift tests.
