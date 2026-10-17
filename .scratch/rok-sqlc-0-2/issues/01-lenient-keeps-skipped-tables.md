# 01: A table skipped by `--lenient` keeps its earlier files

**What to build:** In a `--lenient` run, a table that fails is skipped. Its previously generated resource directory is kept as-is instead of being removed as stale. In that state, `--check` still reports the failure but does not report drift for those files. A table that no longer exists in the schema (dropped, or never created) is still stale. Update the ADR, README and CHANGELOG.

**Blocked by:** None

**Status:** ready-for-agent

- [ ] Generate a table once, break it in a later migration, then regenerate with `--lenient`. Its files are unchanged byte for byte, and the warning names the table (CLI seam).
- [ ] A dropped table is still removed in a lenient run.
- [ ] A strict run that hits the same failure writes nothing.
