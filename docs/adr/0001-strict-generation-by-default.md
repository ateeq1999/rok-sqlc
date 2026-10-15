# Strict generation by default

rok-sqlc refuses to generate when a table contains unsupported constructs: it writes nothing and exits nonzero, naming the offending table and construct. Generation only proceeds for partially unsupported schemas when `--lenient` is passed, in which case skipped tables are reported as `warning:` lines on stderr and the exit code is 0. We chose strict-by-default because a partially generated output directory that silently lacks resources is a worse failure mode than a loud error; most generators do the opposite, so this records the intent to stop the default from being "fixed" later.

Failure diagnostics are evaluated against the selected scope: with `--resource users`, an unrelated unsupported table neither aborts a strict run nor produces warnings.

The all-skipped case is not a successful lenient run: when every table in scope is unsupported, rok-sqlc still exits nonzero (`all tables are unsupported`), because exit 0 would report success while writing nothing — the footgun this ADR exists to prevent.

## Considered Options

- **Lenient by default** (warn + skip, `--strict` to abort) — rejected: silent partial output is a footgun in CI.
