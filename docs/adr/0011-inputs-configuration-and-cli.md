# Inputs, configuration, and CLI: one model, layered sources

Every way of configuring rok-sqlc fills the same `Config` (ADR 0012). The CLI builds it from layered sources, and the most specific source wins: **flags > JSON payload (`-j/--json`) > `rok-sqlc.toml` > defaults**. A source that leaves a key unset defers to the next one down; a key a higher source sets always replaces the lower sources' value for it (a list is replaced whole, never appended: `--schema` and `--queries`, which may be repeated, replace the payload's or file's whole `schema` or `queries` list). Each `schema` entry, from any source, is a SQL file or a directory of migrations read in filename order with `*.down.sql` skipped; each `queries` entry is a query file or a directory of them (ADR 0008).

**`rok-sqlc.toml` is optional and discovered like Cargo.** Without `-c/--config`, the CLI looks for `rok-sqlc.toml` in the current directory and then each parent, and uses the first one it finds. `-c path/to/file.toml` names the file explicitly and turns discovery off. With no file, behavior is exactly the flags-and-defaults behavior of earlier releases. A run with neither `--schema` nor a `schema` key is a usage error that mentions both.

```toml
schema   = ["migrations/"]   # or a single string
queries  = ["queries.sql"]
output   = "src/db"
resource = "products"         # rarely useful in a file, but allowed
```

**Paths in the file resolve relative to the file**, so a config means the same thing from any subdirectory. Paths given as flags or in a JSON payload resolve relative to the current directory, as a user typing them expects.

**The JSON payload has the file's shape.** `-j/--json` takes the same keys as `rok-sqlc.toml`, so scripts and editors can drive rok-sqlc without writing a file: `-j '{"schema":["migrations/"],"output":"src/db"}'` inline, `-j @path/to/config.json` from a file, or `-j -` from stdin. A payload read from `@path` still resolves its paths against the current directory, not the payload file: the payload is a stand-in for flags, and a tool generating it knows the directory it runs in. The payload layers over `rok-sqlc.toml` (discovered or named with `-c`); it does not turn discovery off. Malformed JSON is an error that names the source and the line and column.

**Unknown keys are errors** naming the key and the source (the file, or the JSON payload), so a typo (`ouptut`) can't silently do nothing. Keys for features that have not landed are unknown until the ticket that implements them adds them: accepting and ignoring them would be the same silent no-op. Per-run switches (`--dry-run`, `--check`, `--force`, `--lenient`) are flags only. The `[conventions]` section (ADR 0010) merges key by key, like the top-level keys. `derive` (ADR 0013) is a list like `schema`: `-d/--derive`, which may be repeated, replaces the payload's or file's whole list. The `[relationships]` section (ADR 0014) renames or opts out inferred relationships, one entry per table and inferred name (`[relationships.invoices]` with `created_by_user = "author"`, or `approved_by_user = false`); it merges entry by entry, so a payload's entry for one relationship replaces the file's entry for that relationship and leaves the others. It has no flag. The `[modules]` section renames a table's module, one entry per table (its schema-qualified name or its SQL name, e.g. `"audit.users" = "audit_users"`), so same-named tables of two schemas both generate; the row struct, typed id, and key struct follow the new module name. It merges entry by entry like `[relationships]`, and has no flag. An entry naming no table of the schema, or a name that is not a snake_case Rust identifier, fails the run naming the entry (`modules."audit.users"`).

**Discovery is a CLI concern.** `rok_sqlc::generate(&Config)` never reads `rok-sqlc.toml` or the current directory's parents: a `build.rs` gets exactly the `Config` it built.

**Every command and flag has a short alias** (`g`, `-s`, `-o`, `-r`, `-q`, `-j`, `-c`, `-k`, `-n`, `-f`, `-l`, `-d`, `-h`, `-V`); new flags must take an unused letter, and the CLI tests enforce it. `--help` and `--version` print to stdout and exit 0.

## Considered Options

- **Merge `--schema` with the file's `schema` list.** Rejected: "flags override the file" should hold for every key, and appending makes it impossible to run against a different schema without editing the file.
- **Resolve file paths against the current directory.** Rejected: the same file would mean different things from different subdirectories, which defeats discovery.
- **Discover the file inside `generate(&Config)` too.** Rejected: a library call whose result depends on the process's working directory and a file it never named is surprising in `build.rs`, where Cargo sets the directory.
- **Stop discovery at a repository or workspace root.** Deferred: Cargo-style "nearest file wins" is simple and predictable; a stray file far up the tree is visible with `-c`.
- **Warn on unknown keys instead of failing.** Rejected for the same reason as ADR 0001: a quiet partial success is worse than a loud error.
