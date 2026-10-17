# 16: Convention plumbing, the `rok` preset, and managed columns

**What to build:** A `[conventions]` section (config file and JSON payload) with `preset`, `tenant_column`, `tenant_setting`, `user_setting`, `soft_delete_column`, `managed_columns`, and `version_column`, all off by default. `preset = "rok"` sets the Rok values: `organization_id`, `app.organization_id`, `app.user_id`, `deleted_at`, `["updated_at", "row_version"]`, and `row_version`. Explicit keys override the preset, and an unknown preset is an error. This ticket implements **managed columns**: they are dropped from create inputs and update patches but stay in rows. The other conventions are parsed and validated here and implemented by tickets 17–19. Record the conventions design as ADR 0010.

**Blocked by:** 04, 13, 27

**Status:** resolved

- [x] With `managed_columns = ["updated_at", "row_version"]`, a table using a `touch_row`-style trigger can be created and updated from the fixture crate without supplying those columns, and the row reflects the trigger's values (PostgreSQL seam).
- [x] Without conventions configured, generated code is identical to before (CLI seam: no diff).
- [x] `preset = "rok"` plus an explicit `soft_delete_column` override uses the override.
- [x] An unknown preset or convention key fails with an actionable error.
- [x] ADR 0010 is written.

## Comments

Resolved on integration branch `rok-sqlc-0-1` (final tip b865371). Final code review fixes landed in 8dab99f..b865371. See CHANGELOG.md (Unreleased) and the ADRs for design decisions.
