# 04: `rok-sqlc.toml`: discovery, relative paths, and precedence

**What to build:** An optional `rok-sqlc.toml` holds the same options as the CLI (`schema`, `queries`, `output`, `resource`, and later `derive`, `[conventions]`, and `[relationships]`). Without `-c/--config`, rok-sqlc looks in the current directory and then each parent, like Cargo. Paths inside the file resolve relative to the file. Flags override the file, and the file overrides defaults. Unknown keys are an error naming the key and the file. With no file present, behavior is unchanged. Record inputs, configuration, and CLI decisions as ADR 0011.

**Blocked by:** 02

**Status:** ready-for-agent

- [ ] With a `rok-sqlc.toml` in a parent directory, `rok-sqlc g` run from a subdirectory generates using the file's inputs, resolved against the file's directory (CLI seam).
- [ ] `-c path/to/other.toml` uses that file instead of discovery.
- [ ] A flag overrides the same key from the file.
- [ ] An unknown key fails with an error naming the key and the file.
- [ ] ADR 0011 is written.
