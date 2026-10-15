# Inputs, configuration, and CLI: one model, layered sources

Every way of configuring rok-sqlc fills the same `Config` (ADR 0012). The CLI builds it from layered sources, and the most specific source wins: **flags > JSON payload (`-j/--json`, later) > `rok-sqlc.toml` > defaults**. A source that leaves a key unset defers to the next one down; a flag given on the command line always replaces the file's value for that key (`--schema`, which may be repeated, replaces the file's whole `schema` list, it does not append). Each `schema` entry, from a flag or the file, is a SQL file or a directory of migrations read in filename order with `*.down.sql` skipped.

**`rok-sqlc.toml` is optional and discovered like Cargo.** Without `-c/--config`, the CLI looks for `rok-sqlc.toml` in the current directory and then each parent, and uses the first one it finds. `-c path/to/file.toml` names the file explicitly and turns discovery off. With no file, behavior is exactly the flags-and-defaults behavior of earlier releases. A run with neither `--schema` nor a `schema` key is a usage error that mentions both.

```toml
schema   = ["migrations/"]   # or a single string
queries  = ["queries.sql"]
output   = "src/db"
resource = "products"         # rarely useful in a file, but allowed
```

**Paths in the file resolve relative to the file**, so a config means the same thing from any subdirectory. Paths given as flags (and, later, in a JSON payload) resolve relative to the current directory, as a user typing them expects.

**Unknown keys are errors** naming the key and the file, so a typo (`ouptut`) can't silently do nothing. Keys for features that have not landed (`derive`, `[conventions]`, `[relationships]`) are unknown until the ticket that implements them adds them: accepting and ignoring them would be the same silent no-op. Per-run switches (`--dry-run`, `--check`, `--force`, `--lenient`) are flags only.

**Discovery is a CLI concern.** `rok_sqlc::generate(&Config)` never reads `rok-sqlc.toml` or the current directory's parents: a `build.rs` gets exactly the `Config` it built.

**Every command and flag has a short alias** (`g`, `-s`, `-o`, `-r`, `-q`, `-j`, `-c`, `-k`, `-n`, `-f`, `-l`, `-h`, `-V`); new flags must take an unused letter, and the CLI tests enforce it. `--help` and `--version` print to stdout and exit 0.

## Considered Options

- **Merge `--schema` with the file's `schema` list.** Rejected: "flags override the file" should hold for every key, and appending makes it impossible to run against a different schema without editing the file.
- **Resolve file paths against the current directory.** Rejected: the same file would mean different things from different subdirectories, which defeats discovery.
- **Discover the file inside `generate(&Config)` too.** Rejected: a library call whose result depends on the process's working directory and a file it never named is surprising in `build.rs`, where Cargo sets the directory.
- **Stop discovery at a repository or workspace root.** Deferred: Cargo-style "nearest file wins" is simple and predictable; a stray file far up the tree is visible with `-c`.
- **Warn on unknown keys instead of failing.** Rejected for the same reason as ADR 0001: a quiet partial success is worse than a loud error.
