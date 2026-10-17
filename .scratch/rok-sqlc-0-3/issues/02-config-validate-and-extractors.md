# 02: Configuration for `validate` and `extractors`

**What to build:** The configuration from ADR 0015, with no generation behind it yet:
- `Derive::Validate`: named `validate` in `derive` (config file, JSON payload, `-d/--derive validate`).
- `Config::extractors: Vec<Extractor>` (a new `#[non_exhaustive]` enum `Extractor { Axum, ActixWeb }`, exported from the crate root): the top-level key `extractors = ["axum", "actix_web"]` in the config file and JSON payload, plus a repeatable `--extractor <axum|actix_web>` flag. Layering follows `derive`: a list replaces the lower layer's list whole.
- Run-level checks that fail the run with a clear message: `validate` without `serde`, and `extractors` without `validate`. An unknown name lists the valid ones.
- Document the new key, flag and derive in README.md (the drift tests in `tests/readme.rs` require it, including the `config-reference` block).

The generator may ignore the new options for now. Ticket 03 uses them.

**Blocked by:** None

**Status:** ready-for-agent

- [ ] Config-file, JSON and flag tests for both options, including precedence and the two dependency errors.
- [ ] README drift tests pass.
