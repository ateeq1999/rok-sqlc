# 08: Module fixture harness (`db/modules`)

**What to build:** A permanent test that replaces the ad-hoc corpus check. It generates every **module fixture**'s migrations (and its query file, where one exists) into a fixture crate at `db/modules/<module>/`. It compiles that crate under the generated-code lint gate and checks rustfmt. When `ROK_SQLC_TEST_DATABASE_URL` is set, it applies all fixture migrations to a fresh database in dependency order and smoke-runs `list`, `count`, and `search_after` for every table. Fixture modules that are expected to fail are recorded with their expected diagnostics, as the existing corpus test does.

**Blocked by:** 03

**Status:** resolved

- [x] The harness generates all migration-based module fixtures into `db/modules/<module>/` and the crate compiles with no warnings.
- [x] rustfmt reports no differences across the generated tree (run per module to stay within command-line limits).
- [x] With a database configured, every generated table's reads run successfully against the migrated corpus; without one, the database part is skipped with a message.
- [x] Expected rejections are asserted, not silently skipped.

## Comments

Resolved on integration branch `rok-sqlc-0-1` (final tip b865371). Final code review fixes landed in 8dab99f..b865371. See CHANGELOG.md (Unreleased) and the ADRs for design decisions.
