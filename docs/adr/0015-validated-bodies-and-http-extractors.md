# Validated bodies and HTTP extractors

An API built on the generated code takes create inputs and update patches as JSON, and should reject a bad body before any statement runs, with an answer a client can act on field by field. rok-sqlc already knows many of the rules (a `VARCHAR(80)` column holds at most 80 characters), so it generates them, and it generates the extractors that apply them for axum and actix-web. Both are opt-in, and the extractors are generated into the output rather than shipped as a crate.

**Rules come from the schema, on the `validator` crate.** `derive = ["serde", "validate"]` adds `validator::Validate` to every create input and update patch, with rules derived from the schema: `VARCHAR(n)`/`CHAR(n)` gives `length(max = n)`, counted in characters as PostgreSQL counts them. A single-column `CHECK` against a literal gives a rule too: a comparison or `BETWEEN` gives `range`, and `char_length(col)`/`length(col)` compared with a literal gives `length`. Every other `CHECK` stays the database's alone. Each rule carries an English message naming the column (`"name must be at most 80 characters"`) and validator's stock code (`length`, `range`). `NOT NULL`, enums, and value lists are not rules: the Rust types already reject a missing field or an unknown label while deserializing. A typed id over text gets `validator::ValidateLength`, so a key column keeps its length rule. `validate` without `serde` fails the run. `validator` was chosen over generated plain-Rust `validate()` methods because a hand-written request type, which already uses `validator` in Rok's apps, then goes through the same extractor as a generated input.

**Extractors are generated.** `extractors = ["axum", "actix_web"]` (which requires `validate`) generates `db/http.rs`, with a module per framework: `db::http::axum::ValidatedJson<T>` and `db::http::actix_web::ValidatedJson<T>`, for any `T: DeserializeOwned + Validate`, including hand-written request types. A body is a **validated body** only after it deserializes and passes every rule. Otherwise the extractor rejects with that module's `BodyRejection`:
- `Malformed { message }`, answered `400` with code `MALFORMED_BODY`, for bad JSON syntax or a wrong `Content-Type`;
- `ValidationFailure(validator::ValidationErrors)`, answered `422` with code `VALIDATION_FAILED`, for a failed rule, and for well-formed JSON that doesn't fit the type (a missing field, a wrong type, an unknown label), which carries serde's message and no per-field detail.

Both are answered in the Rok error envelope, `{"success": false, "error": {"message", "code", "details"?}}`, where `details` is validator's own serialization of the errors (`{"name": [{"code": "length", "message": "…", "params": {…}}]}`). `BodyRejection` is public, so an app that wants another body matches on it instead of parsing JSON again. Only JSON bodies are covered: query strings, forms, search filters, and query-file parameter structs are not validated. The `{success, data, meta}` response envelope is a separate decision, not made here.

## Considered Options

- **A companion crate (`rok-sqlc-web`) the generated code depends on.** Rejected: generated code has never needed a rok-sqlc runtime crate (ADR 0013), and a small module the user owns, reads, and can edit beats a dependency whose release cycle is tied to the generator's.
- **Generated plain-Rust `validate()` methods, with no `validator` dependency.** Rejected: one extractor could not then take both generated inputs and the hand-written, `validator`-based request types Rok's apps already have.
- **Leave extractors to each app.** Rejected: every app would rewrite the same two extractors and the same envelope, and malformed bodies would keep going through per-app `JsonConfig` handlers in different shapes.
- **Flattened `details` (`{"name": ["…"]}`).** Rejected: existing clients read validator's shape, and its `params` let a client phrase its own message.
- **Per-field `details` for deserialization errors through `serde_path_to_error`.** Deferred: another dependency for a case the type system already narrows down.

## Consequences

`VARCHAR(n)` lengths and simple `CHECK`s, which the parser dropped before, are now kept on the model. The consuming crate depends on `validator` (with its `derive` feature) and on the framework it lists. The generated `mod.rs` names both.
