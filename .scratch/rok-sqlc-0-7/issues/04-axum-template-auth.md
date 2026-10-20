# 04: M5 auth in the axum template

**What to build:** decision A6, for the M5 features (see `../rok-sqlc-0-6/spec.md` and ADR 0018).

- Port `features/auth/` and `features/account/` to `templates/axum-api/`.
  - `dto.rs` and `service.rs` (with the `AuditTrail` port) are byte-identical to actix's.
  - `routes.rs`/`handlers.rs` are rewritten for axum, with the rate-limit layer on the `/auth` routes and `/auth/me`, and `account` mounted before the generated `users` routes so `/users/me` isn't captured by `/users/{id}` (check axum's router semantics: static segments win over captures there, so say what was needed).
- `AuthUser`/`AdminUser` load the session state per request (Q21), as `FromRequestParts` over `AppState`.
- The `AuditLog` port implementation in `app.rs`.
- Port every PostgreSQL flow test (`sign_up`, `login`, `account`, `password`, `email_change`, `invitations`, `auth`, `mail`) with the same assertions.
- The contract test covers all 13 M5 paths.
- Extend the drift test to `features/auth/{dto,service}.rs`, `features/account/{dto,service}.rs` and the `audit_log_entry/service.rs` `create_in`.

**Blocked by:** 03

**Status:** ready-for-agent

- [ ] Every ported flow test passes on PostgreSQL. The contract test covers the M5 paths.
- [ ] The drift test covers the auth files.
- [ ] Gates pass.
