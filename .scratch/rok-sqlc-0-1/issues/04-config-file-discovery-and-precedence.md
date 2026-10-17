# 04: `rok-sqlc.toml`: discovery, relative paths, and precedence

**What to build:** An optional `rok-sqlc.toml` holds the same options as the CLI (`schema`, `queries`, `output`, `resource`, and later `derive`, `[conventions]`, and `[relationships]`). Without `-c/--config`, rok-sqlc looks in the current directory and then each parent, like Cargo. Paths inside the file resolve relative to the file. Flags override the file, and the file overrides defaults. Unknown keys are an error naming the key and the file. With no file present, behavior is unchanged. Record inputs, configuration, and CLI decisions as ADR 0011.

**Blocked by:** 02

**Status:** resolved

- [x] With a `rok-sqlc.toml` in a parent directory, `rok-sqlc g` run from a subdirectory generates using the file's inputs, resolved against the file's directory (CLI seam).
- [x] `-c path/to/other.toml` uses that file instead of discovery.
- [x] A flag overrides the same key from the file.
- [x] An unknown key fails with an error naming the key and the file.
- [x] ADR 0011 is written.

## Comments

Resolved on integration branch `rok-sqlc-0-1` (final tip b865371). Final code review fixes landed in 8dab99f..b865371. See CHANGELOG.md (Unreleased) and the ADRs for design decisions.
