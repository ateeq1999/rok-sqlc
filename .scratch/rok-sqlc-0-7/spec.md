# rok-sqlc 0.7: generated APIs and a template for axum

## Problem

`rok-sqlc` generates API features only for actix-web, and `init --template axum` answers "unknown template". ADR 0016 promised axum as the second framework against the same contract, and the target contract comes from an axum starter.

## Decisions

The authority is `notes/axum-decisions.md` (A1–A8, settled 2026-10-20). See also ADR 0016, ADR 0017 and ADR 0018.

In short, a project picks `[api] framework = "axum"`. The generator then emits axum routes, handlers and extractors over the same DTOs, services, repositories and data layer, with identical wire answers. `templates/axum-api/` mirrors the actix template, M5 auth included, and is checked against the same contract.

## Tickets

`issues/01`…`05`. Dependency graph: 01 → (02 ∥ 03) → 04 → 05.

## Out of scope

M6 (2FA, API keys, media and avatars) comes after 0.7 for both templates.
