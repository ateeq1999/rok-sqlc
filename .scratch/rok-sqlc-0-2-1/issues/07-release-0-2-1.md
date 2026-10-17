# 07: Release 0.2.1

**What to build:** Prepare 0.2.1:
- bump the version to 0.2.1;
- move CHANGELOG's Unreleased entries under 0.2.1;
- update the README's limitations, the supported SQL, and the type table;
- regenerate `docs/examples` if they change;
- run `cargo publish --dry-run`.

Do NOT publish. The maintainer publishes.

**Blocked by:** 01, 02, 03, 04, 05, 06, 08

**Status:** resolved

- [x] Version 0.2.1 everywhere it is declared, and the CHANGELOG lists every ticket's user-visible change.
- [x] `cargo publish --dry-run` passes, and the README drift tests pass.

## Comments

Done on branch `rok-sqlc-0-2-1`. See CHANGELOG.md [0.2.1].
