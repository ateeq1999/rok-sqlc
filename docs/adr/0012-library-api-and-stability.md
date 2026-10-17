# Library API and stability: one stable door

The crate's only stable library API is `rok_sqlc::generate(&Config) -> Result<Report>` and the types it takes and returns, re-exported at the crate root: `Config` (with `Conventions` and its `IdGeneration`, ADR 0010; `Derive`, ADR 0013; `RelationshipChange`, ADR 0014; and `Extractor`, ADR 0015), `Report` (with `RefusedFile`, `TableFailure`, `QueryFailure`), `Error` (with `Error::report()`), and the `Result<T, E = Error>` alias `generate` returns. It does exactly what `rok-sqlc generate` does, because the CLI is a thin layer over it: the CLI parses flags into a `Config`, calls `generate`, and prints the `Report`. A `build.rs` therefore gets the same files and the same failures as the command line.

```rust
// build.rs
let mut config = rok_sqlc::Config::default();
config.schema = vec!["migrations/0001_init.sql".into()];
config.output = "src/db".into();
rok_sqlc::generate(&config)?;
```

**The library never prints.** Skipped tables and queries, written, unchanged, and refused files all come back in the `Report`; the CLI turns them into its `warning:`/`error:`/`generated` lines. A failed run returns an `Error` whose `report()` holds what was known when it stopped (the unsupported tables or queries, or the refused files), so a caller can show the same detail the CLI shows. `Error` is the crate's own type: `anyhow` stays an implementation detail.

**Everything else is hidden.** Internal modules are crate-private, or `#[doc(hidden)]` where the `rok-sqlc` binary or the crate's own tests need them (`cli`, `parser`, `queries`). Hidden items carry no stability promise and may change in any release.

The same holds for `#[doc(hidden)]` items in *generated* code: the `push` methods of the search conditions, `push_null_check`, `open_exists`, and `correlate` in `db::search`. Only other generated modules call them. They are `pub` rather than `pub(crate)` because `db::search` is generated whole for every schema: a crate whose schema has no nullable column or no relationship would never call some of them, and `pub(crate)` would then trip `dead_code` under the consuming crate's `#![deny(warnings)]`. Hidden generated items are not part of the generated API, so changing them is not a breaking change.

**Growth without breakage.** `Config`, `Conventions`, `Report`, and the failure types are `#[non_exhaustive]`: new options and report details (for example source `path:line`) arrive in minor releases. Callers build a `Config` from `Config::default()` and assign fields.

**What counts as breaking.** Changing the meaning of a `Config` field or the shape of `generate` is breaking. So is any change to the *generated* API (renamed functions, changed signatures, different types): regenerating after an upgrade must not break the user's code in a minor release. Generated code targets SQLx 0.9 only (0.9.0 and later 0.9.x), so it needs Rust 1.94, SQLx 0.9's minimum supported Rust version. Moving generated code to another SQLx version is a breaking change. The tool itself has a lower floor: its minimum supported Rust version is 1.88, because it does not depend on SQLx. Raising either floor is a minor-release change announced in the changelog.

## Considered Options

- **Keep the internal modules public as the library API.** Rejected: every parser and generator refactor would be a breaking change, and users would reimplement the pipeline (as the fixture test once did) instead of getting the CLI's behavior.
- **Return `anyhow::Result`.** Rejected: it ties the public signature to a dependency and cannot carry the report of a failed run. A `std::error::Error` type works with `?` in any `build.rs`, including with `anyhow` and `Box<dyn Error>`.
- **A builder for `Config`.** Deferred: public fields plus `#[non_exhaustive]` and `Default` give the same forward compatibility with less API. A builder can be added later without breaking anyone.
- **Supporting several SQLx versions behind features.** Rejected for 0.1: each version doubles the generated-code test matrix for little demand.
