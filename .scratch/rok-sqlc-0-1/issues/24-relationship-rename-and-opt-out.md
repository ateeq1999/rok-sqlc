# 24: Relationship rename and opt-out in configuration

**What to build:** A `[relationships]` section (config file and JSON payload) can rename any inferred relationship, identified by table and relationship name, and opt one out. Renames apply to loaders, joined reads, and relationship filters alike. Configuration that names a relationship that doesn't exist, or a rename that collides with another name, fails with an actionable error. This is ADR 0002's declared lane, limited to rename and opt-out.

**Blocked by:** 20, 04

**Status:** resolved

- [x] Renaming `invoices.created_by_user` to `author` changes the loader, joined read, and filter names, and they work against PostgreSQL (PostgreSQL seam).
- [x] An opted-out relationship generates nothing on either side.
- [x] Unknown relationships and colliding renames fail with errors naming the config location (CLI seam).

## Comments

Resolved on integration branch `rok-sqlc-0-1` (final tip b865371). Final code review fixes landed in 8dab99f..b865371. See CHANGELOG.md (Unreleased) and the ADRs for design decisions.
