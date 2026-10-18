# 01: The wire standard's error body in `db/http`

**What to build:** The 0.3 extractors (`db/http/{mod,axum,actix_web}.rs`) answer `{"success": false, "error": {"message", "code", "details"?}}` with UPPER codes. Change them to the wire standard in ADR 0016: `{"error": {"code", "message", "details"?}}` with snake_case codes. No `success` field.

| Case | Status | Code |
|---|---|---|
| bad JSON syntax, wrong `Content-Type`, unreadable body | 400 | `bad_request` |
| body over the limit | 413 | `payload_too_large` |
| data error or failed rule | 422 | `validation_failed` (with validator's `details` for rules) |

Keep both frameworks' bodies identical. Add the error body's serializable types to the shared module so ticket 04's `AppError` and ticket 05's `shared/response.rs` can reuse them. Rename them if the names clash; record the final names in the ticket comments.

This is a breaking change for 0.3 users of `extractors`. Add it under `[Unreleased]` → "Breaking changes to the generated API" in the CHANGELOG, and update the README section and the `docs/examples/validated-api` example. ADR 0015 already carries a note that ADR 0016 supersedes this part.

**Blocked by:** None

**Status:** resolved

- [x] Fixture tests (axum and actix-web) assert the new body, status and code for every rejection, and that both frameworks match.
- [x] README drift tests and the example pass.

## Comments

Resolved on branch `rok-sqlc-0-4-01`.

The shared error-body types stay in `db::http` (`db/http/mod.rs`), with no name clash against ticket 04's `AppError` or ticket 05's `shared/response.rs`:

- `ErrorBody { error: ErrorPayload }`, plus `ErrorBody::new(code, message)` for any other error answer (no `details`).
- `ErrorPayload { code: String, message: String, details: Option<validator::ValidationErrors> }`, serialized in that order; `details` is skipped when `None`.
- Code constants: `BAD_REQUEST = "bad_request"` (renamed from `MALFORMED_BODY`), `PAYLOAD_TOO_LARGE = "payload_too_large"`, `VALIDATION_FAILED = "validation_failed"`.

`BodyRejection` keeps its variants (`Malformed`, `TooLarge`, `ValidationFailure`). The example test `docs/examples/validated-api/tests/envelope.rs` is renamed `error_body.rs`.
