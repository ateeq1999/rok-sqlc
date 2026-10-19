# rok-sqlc 0.5: validation rules for generated APIs

## Problem

A generated API reports a duplicate email as `409 conflict` naming a database constraint, and a dangling foreign key the same way. It has no way to check formats the schema can't express, such as email, URL or phone. Clients want field-level answers they can show next to an input.

## Decisions

Settled with the maintainer on 2026-10-19 (Q51–Q54, recorded in `../rok-sqlc-0-4-m3/notes/next-milestones-decisions.md`, the authority for this spec). Also see ADR 0016 (generated APIs) and ADR 0015 (validated bodies).

- **`unique` (automatic, from the schema):** every single-column unique key gets a pre-check in `create_in`/`update_in`. A taken value answers `422 validation_failed` with `{"<column>": [{"code": "unique", "message": "<column> is already taken"}]}`.
  - Soft-deleted rows don't count, and the check stays within the tenant.
  - On update, the row itself is excluded.
  - Unique indexes on `lower(col)` are recognised, and checked case-insensitively.
  - A composite unique key is reported against its last column, with the message naming every column.
  - The database constraint stays as the race fallback (`409 conflict`).
- **`exists` (automatic, from foreign keys):** every foreign-key field in a create or update body gets a pre-check that the referenced row exists. A missing row answers 422 with `{"<column>": [{"code": "exists", "message": "<column> does not refer to an existing <singular>"}]}`.
  - A row the caller can't see counts as missing (owner and tenant scoping, soft delete).
  - The FK constraint stays as the race fallback (`409`).
- **String rules (`[api.rules]`, VineJS names):**
  - **Validation rules:** `email`, `url`, `uuid`, `ulid`, `ipAddress`, `mobile`, `alpha`, `alphaNumeric`, `ascii`, `regex`, `in`, `notIn`, `startsWith`, `endsWith`, `minLength`, `maxLength`, `fixedLength`, `hexCode`, `iban`, `jwt`, `postalCode`, `creditCard` and `coordinates`.
  - **Transforms:** `trim`, `toLowerCase`, `toUpperCase` and `normalizeEmail` run before validation, and the transformed value is what's stored.
  - **Not included:** `confirmed`, `sameAs`, `notSameAs` and `activeUrl`.
  - **Syntax:** a rule is a string (`"email"`, `"minLength(3)"`, `"alphaNumeric(allowDashes)"`, `"in(SA, AE, EG)"`), or an inline table where a string can't carry the argument (`{ regex = "^[a-z0-9-]+$" }`).
  - **Failing checks:** an unknown rule, bad arguments, a rule on a non-text column, or a rule rok-sqlc can't give VineJS's meaning fails the config check with a clear message.

### Defaults taken without asking (reversible)

- **Order:** format rules (and the schema-derived length/range rules) run first. If any fail, the 422 lists those. Then the `unique` and `exists` pre-checks run, inside the write's transaction, and every database-check failure is listed in one 422.
- **`mobile`:** uses `validator`-style mobile checks with an optional locale (`mobile(ar-SA)`). VineJS's `phone` comes from a plugin and isn't included.
- **Messages:** English, naming the column, as the schema-derived rules do.

## Version

Turning a constraint's `409` into a field-level `422` changes the generated API, so this is **0.5.0**, listed under "Breaking changes to the generated API".

## Constraints

These are unchanged:
- the tool builds on Rust 1.88;
- generated code needs 1.94, passes the lint gates, `check-arch` and rustfmt, and adds no unwrap, expect or panic;
- fresh `init` and `init --template` projects build clippy-clean;
- output without `[api]` is byte-identical;
- the template's contract test passes;
- CI runs Debian PostgreSQL 17.

## Tickets

`issues/01` to `issues/05`:
- 01, 03 and 04 can start together;
- 02 needs 01 and 03;
- 05 needs all of them.
