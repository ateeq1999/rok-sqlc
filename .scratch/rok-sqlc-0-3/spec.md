# rok-sqlc 0.3: validated bodies and HTTP extractors

## Problem

An API built on the generated code takes create inputs and update patches as JSON. Each app writes its own extractor and error envelope to reject a bad body before any statement runs. rok-sqlc already knows many of the rules (a `VARCHAR(80)` column holds at most 80 characters) but drops them.

## Decisions

Settled with the maintainer on 2026-10-17 and recorded in `docs/adr/0015-validated-bodies-and-http-extractors.md`, which is the authority. The terms are in `GLOSSARY.md`: **validated body**, **validation rule**, **validation failure**.

In brief:
- `derive = ["serde", "validate"]` adds `validator::Validate` (validator 0.21) to create inputs and update patches, with schema-derived rules and English messages.
- `extractors = ["axum", "actix_web"]` generates `db/http/`, with a `ValidatedJson<T>` and a `BodyRejection` per framework (axum 0.8, actix-web 4), answering in the Rok error envelope.
- Only JSON bodies are covered. The response envelope (`{success, data, meta}`) is out of scope.
- Release 0.3.0. The integration branch `rok-sqlc-0-3` is based on `rok-sqlc-0-2-1`.

## Constraints

These are unchanged:
- the tool builds on Rust 1.88;
- generated code needs Rust 1.94 and passes the lint gate (ADR 0013), with every public item documented;
- a run without the new keys generates byte-for-byte what 0.2.1 generates;
- the README drift tests pass (every flag and config key documented);
- CI runs on Linux, macOS and Windows.

## Tickets

`issues/01` to `issues/06`. 01 and 02 can run in parallel. 03 needs both. 04 needs 03. 05 needs 04. 06 needs all of them.
