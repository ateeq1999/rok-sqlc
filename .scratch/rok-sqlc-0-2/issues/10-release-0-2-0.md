# 10: Release 0.2.0

**What to build:** Prepare 0.2.0 for release:
- bump the version to 0.2.0;
- move CHANGELOG's Unreleased entries under 0.2.0, with a Breaking section;
- update the README's limitations and roadmap;
- regenerate `docs/examples`;
- run `cargo publish --dry-run`.

Do NOT publish. The maintainer publishes.

**Blocked by:** 01, 02, 03, 04, 05, 06, 07, 08, 09

**Status:** resolved

- [x] Version 0.2.0 everywhere it is declared, and the CHANGELOG lists every ticket's user-visible change.
- [x] `cargo publish --dry-run` passes, and the README drift tests pass.

## Comments

Resolved on integration branch `rok-sqlc-0-2`. Review fixes in 760d008..e2255b5, large-file refactor up to 6fa10e3, release prep in the 0.2.0 commit. See CHANGELOG.md [0.2.0] and the ADRs.
