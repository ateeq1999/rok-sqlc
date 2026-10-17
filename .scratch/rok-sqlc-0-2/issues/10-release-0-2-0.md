# 10: Release 0.2.0

**What to build:** Prepare 0.2.0 for release:
- bump the version to 0.2.0;
- move CHANGELOG's Unreleased entries under 0.2.0, with a Breaking section;
- update the README's limitations and roadmap;
- regenerate `docs/examples`;
- run `cargo publish --dry-run`.

Do NOT publish. The maintainer publishes.

**Blocked by:** 01, 02, 03, 04, 05, 06, 07, 08, 09

**Status:** ready-for-agent

- [ ] Version 0.2.0 everywhere it is declared, and the CHANGELOG lists every ticket's user-visible change.
- [ ] `cargo publish --dry-run` passes, and the README drift tests pass.
