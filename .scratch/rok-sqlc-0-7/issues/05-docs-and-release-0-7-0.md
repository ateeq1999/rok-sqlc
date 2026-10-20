# 05: Docs, review fixes, and 0.7.0

**What to build:**
- **ADR 0019:** axum support (A1–A8). It covers what differs per framework, `Services` + `FromRef`, why there's no `utoipa-axum`, the drift test between templates, and the rate-limit layer.
- **Guide:** the pages that show actix code (getting started, architecture, configuring, auth, wire standard, troubleshooting) gain axum where it differs (`[api] framework`, `init --framework axum`, `init --template axum`, handler and extractor examples). Keep the shared parts written once.
- **README:** templates section, configuration, `init` options, and the roadmap (axum done; M6 next for both templates).
- **0.7.0:**
  - bump the version everywhere (precedent: commit a191d97);
  - CHANGELOG `[0.7.0]` with breaking changes first, if any (for example, the generated root's shape changing for actix);
  - `cargo publish --dry-run --locked`.

Do NOT tag or publish.

**Blocked by:** 01, 02, 03, 04

**Status:** ready-for-agent

- [ ] Drift tests, and the full suite on Rust 1.94 against Debian PostgreSQL.
- [ ] The 1.88 build and the publish dry-run.
