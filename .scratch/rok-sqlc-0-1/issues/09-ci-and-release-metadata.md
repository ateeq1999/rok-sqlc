# 09: CI, license files, crates.io metadata, CHANGELOG, and `rust-version`

**What to build:** GitHub Actions run on every push and pull request:
- **Linux:** fmt check, clippy `-D warnings`, the full test suite, and the PostgreSQL integration and module fixture suites against a PostgreSQL service container, on Rust 1.88 and stable.
- **macOS and Windows:** build and test without PostgreSQL.

The crate gains `LICENSE-MIT` and `LICENSE-APACHE`, crates.io metadata (`repository`, `keywords`, `categories`, `readme`, `rust-version = "1.88"`), and a `CHANGELOG.md` with an Unreleased section recording the changes so far.

**Blocked by:** 08

**Status:** ready-for-agent

- [ ] The workflow file defines the jobs above, and the PostgreSQL job sets `ROK_SQLC_TEST_DATABASE_URL` for the service container.
- [ ] `cargo +1.88 build` and `cargo +1.88 test` pass locally (or the MSRV is corrected with a reason).
- [ ] `cargo package --list` includes the license files and README, and excludes `.scratch/`, `modules/`, and test fixtures that aren't needed.
- [ ] CHANGELOG lists the breaking generated-API changes made so far (singular names, `Limit`, and so on).
