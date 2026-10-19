# Decisions for the milestones after 0.4.0 (Q51–Q55, 2026-10-19)

These were settled with the maintainer and are the input for the next milestones' specs. They aren't built in 0.4.0, except `[api.immutable]` (ticket 05 of milestone 3).

## Q51: Order

0.4.0 ships with `[api.immutable]` only. The rules below go into the next milestone. Verification flows go into milestone 5 (auth).

## Q52: The `unique` rule (automatic, from the schema)

Every single-column unique key gets a pre-check in `create`/`update`. It answers `422 validation_failed` with field details (`{"email": [{"code": "unique", "message": "email is already taken"}]}`) instead of `409 conflict` naming a constraint.
- **Race fallback:** the database constraint remains, still answering 409.
- **Scope:** soft-deleted rows don't count, and the check stays within the tenant.
- **Expression indexes:** rok-sqlc starts recognizing unique indexes on `lower(col)` (it ignores expression indexes today). Such a column counts as unique, and its check compares case-insensitively. The template's `users_email_key ON lower(email) WHERE deleted_at IS NULL` is the motivating case.
- **Composite unique keys:** reported against their last column, with a message naming every column.

## Q53: The `exists` rule (automatic, from foreign keys)

Every foreign-key field in a create or update body gets a pre-check that the referenced row exists. A missing row answers 422 with `{"category_id": [{"code": "exists", "message": "category_id does not refer to an existing category"}]}`.
- **Ownership and tenancy:** a row the caller can't see counts as missing, so nothing leaks.
- **Race fallback:** the FK constraint answers 409.

## Q54: String rules (config, VineJS names)

```toml
[api.rules.users]
email   = ["email", "trim", "toLowerCase"]
phone   = ["mobile"]
website = ["url"]
handle  = ["alphaNumeric(allowDashes)", "minLength(3)"]
country = ["in(SA, AE, EG)"]
```

- **Rules:** `email`, `url`, `uuid`, `ulid`, `ipAddress`, `mobile`, `alpha`, `alphaNumeric`, `ascii`, `regex`, `in`, `notIn`, `startsWith`, `endsWith`, `minLength`, `maxLength`, `fixedLength`, `hexCode`, `iban`, `jwt`, `postalCode`, `creditCard`, `coordinates`.
- **Transforms:** `trim`, `toLowerCase`, `toUpperCase` and `normalizeEmail` run before validation, and the transformed value is what's stored.
- **Left out:** `confirmed`, `sameAs` and `notSameAs` (cross-field rules belong in hand-written request types), and `activeUrl` (network calls during validation).
- **Mapping:** each rule maps to `validator` built-ins or a generated custom function. A rule rok-sqlc can't express with the same meaning fails the config check.

Reference: https://vinejs.dev/docs/types/string. `unique` and `exists` in VineJS come from Lucid; here they come from the schema (Q52, Q53).

## Q55: Verification flows (milestone 5)

Email change (send a code or link to the new address, then confirm) and password change (check the current password, then hash the new one) are flows, not updates. They need a mailer port, token storage and hashing, which is milestone 5's auth surface. Design `[api.verifiable]`, or whatever it ends up being called, in milestone 5's design session, with these two flows as its first requirements.

Until then, a flow's column is immutable in the CRUD (`[api.immutable]`, as `email` is in the template).

**Superseded (2026-10-19):** milestone 5 dropped `[api.verifiable]`. Both flows are hand-written template features (see `../../rok-sqlc-0-6/notes/m5-auth-decisions.md`, Q1).
