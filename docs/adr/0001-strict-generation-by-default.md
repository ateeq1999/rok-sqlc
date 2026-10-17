# Strict generation by default

rok-sqlc refuses to generate when a table contains unsupported constructs: it writes nothing and exits nonzero, naming the offending table and construct. Generation only proceeds for partially unsupported schemas when `--lenient` is passed, in which case skipped tables are reported as `warning:` lines on stderr and the exit code is 0. We chose strict-by-default because a partially generated output directory that silently lacks resources is a worse failure mode than a loud error; most generators do the opposite, so this records the intent to stop the default from being "fixed" later.

Failure diagnostics are evaluated against the selected scope: with `--resource users`, an unrelated unsupported table neither aborts a strict run nor produces warnings.

The all-skipped case is not a successful lenient run: when every table in scope is unsupported, rok-sqlc still exits nonzero (`all tables are unsupported`), because exit 0 would report success while writing nothing — the footgun this ADR exists to prevent.

A skipped table keeps what an earlier run generated for it. When a table the schema still defines fails (a later migration breaks it), its resource directory is kept byte for byte and `db/mod.rs` keeps declaring it, so the consuming crate still builds against the last version that generated. Those files are not stale: a lenient `--check` reports the failure as a warning but no drift for them. A table the schema no longer defines (dropped, or never created) has no failure, and its files are stale and removed as before. Deleting the last good files of a table that merely failed would turn a warning into a broken build, which is the opposite of what `--lenient` asks for. A strict run that hits the same failure still writes nothing.

A statement the SQL parser can't read is treated the same way when rok-sqlc can tell what it alters (0.2). An input that doesn't parse as a whole is read statement by statement, split at top-level semicolons: an unreadable `ALTER TABLE` on a known table (`ALTER COLUMN … SET STATISTICS`, `DROP IDENTITY`, …) fails that table at its `path:line`, so `--lenient` skips it and reads on, while any other unreadable statement still stops the run, at its `path:line`, because its effect on the model is unknown. The split doesn't understand `BEGIN ATOMIC` function bodies, so such a body in an input that fails to parse stops the run too.

## Considered Options

- **Lenient by default** (warn + skip, `--strict` to abort) — rejected: silent partial output is a footgun in CI.
