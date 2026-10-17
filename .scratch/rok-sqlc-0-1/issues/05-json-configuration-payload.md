# 05: `-j/--json` configuration payload

**What to build:** `-j/--json` takes configuration with the same shape as `rok-sqlc.toml`, given as an inline JSON string, `@path` for a file, or `-` for stdin. Precedence is flags > JSON payload > `rok-sqlc.toml` > defaults. Relative paths in the payload resolve against the current directory. Unknown keys and malformed JSON are errors that say what was wrong.

**Blocked by:** 04

**Status:** resolved

- [x] `rok-sqlc g -j '{"schema":["migrations/"],"output":"out"}'` generates the same files as the equivalent flags (CLI seam).
- [x] `-j @config.json` and `-j -` (piped stdin, binary invocation) work.
- [x] A payload value overrides `rok-sqlc.toml`, and a flag overrides the payload.
- [x] Unknown keys and malformed JSON fail with actionable messages and write nothing.

## Comments

Resolved on integration branch `rok-sqlc-0-1` (final tip b865371). Final code review fixes landed in 8dab99f..b865371. See CHANGELOG.md (Unreleased) and the ADRs for design decisions.
