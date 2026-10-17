# 01: A table skipped by `--lenient` keeps its earlier files

**What to build:** In a `--lenient` run, a table that fails is skipped. Its previously generated resource directory is kept as-is instead of being removed as stale. In that state, `--check` still reports the failure but does not report drift for those files. A table that no longer exists in the schema (dropped, or never created) is still stale. Update the ADR, README and CHANGELOG.

**Blocked by:** None

**Status:** resolved

- [x] Generate a table once, break it in a later migration, then regenerate with `--lenient`. Its files are unchanged byte for byte, and the warning names the table (CLI seam).
- [x] A dropped table is still removed in a lenient run.
- [x] A strict run that hits the same failure writes nothing.

## Comments

Resolved on integration branch `rok-sqlc-0-2`. Review fixes in 760d008..e2255b5, large-file refactor up to 6fa10e3, release prep in the 0.2.0 commit. See CHANGELOG.md [0.2.0] and the ADRs.
