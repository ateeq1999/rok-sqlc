# rok-sqlc 0.2: behaviour fixes and the 0.1 limitations

## Problem

rok-sqlc 0.1 is published. The README lists its known limitations, and the 0.1 build left some behaviour questions that the maintainer has now decided. This spec turns both into one release, 0.2.0.

## Decisions (maintainer, 2026-10-17)

- **Lenient runs:** a table skipped by `--lenient` keeps the files an earlier run generated for it. Those files are not stale.
- **Tenant column:** on tenant-scoped tables it is no longer a search filter field or a sort key. This is a breaking change to the generated API (ADR 0012).
- **Managed columns:** a managed column fails its table with a `path:line` diagnostic when it has no source of values (ADR 0001, strict by default). That means a `NOT NULL` column with no `DEFAULT`, that isn't identity or generated, on a table with no `BEFORE INSERT` trigger.
- **Versioned empty patch:** no change. It still returns `VersionConflict` when the version is stale.

## Scope

The tickets in `issues/` cover:
- the decisions above;
- the type and SQL gaps from the README's "Known limitations in 0.1";
- a release ticket.

Everything under "Not planned for 0.1" stays out of scope.

## Constraints

These are unchanged from 0.1:
- the tool builds on Rust 1.88;
- generated code targets SQLx 0.9 and needs Rust 1.94;
- generated code passes the lint gate (ADR 0013);
- the module fixture harness and the README drift tests pass;
- CI runs on Linux, macOS and Windows.
