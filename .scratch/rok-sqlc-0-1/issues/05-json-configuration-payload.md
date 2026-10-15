# 05: `-j/--json` configuration payload

**What to build:** `-j/--json` takes configuration with the same shape as `rok-sqlc.toml`, given as an inline JSON string, `@path` for a file, or `-` for stdin. Precedence is flags > JSON payload > `rok-sqlc.toml` > defaults. Relative paths in the payload resolve against the current directory. Unknown keys and malformed JSON are errors that say what was wrong.

**Blocked by:** 04

**Status:** ready-for-agent

- [ ] `rok-sqlc g -j '{"schema":["migrations/"],"output":"out"}'` generates the same files as the equivalent flags (CLI seam).
- [ ] `-j @config.json` and `-j -` (piped stdin, binary invocation) work.
- [ ] A payload value overrides `rok-sqlc.toml`, and a flag overrides the payload.
- [ ] Unknown keys and malformed JSON fail with actionable messages and write nothing.
