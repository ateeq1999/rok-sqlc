# 07: Stale-file removal and `generate --check`

**What to build:** After a successful run, rok-sqlc removes **stale files**: files under the output directory that carry the rok-sqlc header but are not part of this run's plan, along with directories left empty. Files without the header are never touched. `-k/--check` writes and removes nothing. It reports missing, changed, and stale files, and exits 1 if there is any **drift**, 0 otherwise. `--dry-run` also reports what would be removed.

**Blocked by:** 02

**Status:** resolved

- [x] After a table is dropped from the schema, regenerating removes its resource directory (CLI seam).
- [x] A hand-written file in the output directory survives regeneration.
- [x] `--check` exits 0 on a freshly generated tree, and exits 1 listing the file after a schema change, a hand edit to a generated file, or a dropped table.
- [x] `--check` never modifies the filesystem.

## Decisions

- **`--resource` and stale removal.** Resolved from the spec: stale = generated files not in the plan, with no exception for filtered runs. A filtered run's plan already includes every previously generated resource still in the schema (the `include` merge), so a `--resource` run never removes another live resource; it does remove resources whose table was dropped. Pinned by `filtered_runs_keep_other_generated_resources`.
- A kept `queries.rs` (previous run used `--queries`, this one doesn't) counts as planned and is not stale.
- Library: `Config::check`; drift fails `generate` with an `Error` whose report lists `missing`, `changed`, `stale` (same failure as the CLI, per ADR 0012). `Report::stale` lists removed files (dry run: files that would be removed).
- In check mode a planned path holding a hand-written file is reported as `changed`. Only the generated header marks a file as removable; symlinks are never followed or removed; the output directory itself is never removed.

## Comments

Resolved on integration branch `rok-sqlc-0-1` (final tip b865371). Final code review fixes landed in 8dab99f..b865371. See CHANGELOG.md (Unreleased) and the ADRs for design decisions.
