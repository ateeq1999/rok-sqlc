# 07: Stale-file removal and `generate --check`

**What to build:** After a successful run, rok-sqlc removes **stale files**: files under the output directory that carry the rok-sqlc header but are not part of this run's plan, along with directories left empty. Files without the header are never touched. `-k/--check` writes and removes nothing. It reports missing, changed, and stale files, and exits 1 if there is any **drift**, 0 otherwise. `--dry-run` also reports what would be removed.

**Blocked by:** 02

**Status:** ready-for-agent

- [ ] After a table is dropped from the schema, regenerating removes its resource directory (CLI seam).
- [ ] A hand-written file in the output directory survives regeneration.
- [ ] `--check` exits 0 on a freshly generated tree, and exits 1 listing the file after a schema change, a hand edit to a generated file, or a dropped table.
- [ ] `--check` never modifies the filesystem.
