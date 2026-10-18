# 04: Prepare 0.4.0

**What to build:** Prepare the 0.4.0 release, following the 0.2.1 and 0.3.0 precedent:
- the version goes to 0.4.0 in `Cargo.toml` and `Cargo.lock` (`cargo update -p rok-sqlc --offline`), the README status line, the install script examples, and the release workflow's example tag;
- CHANGELOG `[Unreleased]` → `[0.4.0] - <date>`, with the compare links updated. The breaking-changes section comes first (the wire standard's error body for `extractors`, from milestone 1). The section covers milestones 1 to 3;
- the README's limitations and roadmap are brought up to date: milestones 4 (axum) and 5 (auth endpoints, uploads, api-keys) become "planned".
- `cargo publish --dry-run --locked`.

Do NOT tag, push the tag, or publish. The maintainer pushes `v0.4.0`, and the Release workflow publishes.

**Blocked by:** 02, 03

**Status:** ready-for-agent

- [ ] Version 0.4.0 everywhere it is declared, and the CHANGELOG covers every user-visible change of milestones 1 to 3.
- [ ] The full suite passes (Debian PostgreSQL, Rust 1.94), plus the 1.88 build and the publish dry run.
