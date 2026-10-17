# rok-sqlc 0.2.1: the limitations rok-sqlc can lift on its own

## Problem

The README's "Limitations and roadmap" lists what 0.2.0 can't do. Some of these limits come from the SQL parser (sqlparser 0.59; 0.63 reads none of them either), some are rok-sqlc's own, and some are design decisions. This release lifts the ones rok-sqlc can fix without a parser change or a breaking change to the generated API.

## Decisions

- **Additive only.** Nothing that generates today changes shape. Schemas and queries that failed now generate.
- **Statements the parser can't read** are recognised at the token level when their effect on the model is known: either they don't change the model, or they map onto an operation the parser does read.
- **Out of scope** (they stay in the README): serde for `INTERVAL`, `TIMETZ` and the new SQLx-only types (`PgMoney`, `PgRange`), because a fix would mean new wrapper types; keyset pagination off the primary key; array and JSON operators in filters; the relationship-depth cap; typed ids across migration sets; the lenient last-good shape; the versioned-update race; `UNION` outside a recursive CTE; data-modifying CTEs; `ALTER TABLE … RENAME TO`, `SET SCHEMA` and partitioning.

## Scope

The tickets in `issues/` cover:
- `ALTER TABLE` and `ALTER COLUMN` forms the parser can't read, or reads but rok-sqlc rejects, that don't change the model or map onto ones that do;
- `BEGIN ATOMIC` bodies, and other unreadable statements that can't change the model;
- index, sequence, view and constraint renames;
- partial-index `ON CONFLICT` targets in query files;
- `WITH RECURSIVE` in query files;
- `MONEY`, range, and network types;
- primary key ids made by the generated code (ticket 08, asked for during the work);
- a release ticket.

## Constraints

These are unchanged from 0.2:
- the tool builds on Rust 1.88;
- generated code targets SQLx 0.9 and needs Rust 1.94;
- generated code passes the lint gate (ADR 0013);
- the module fixture harness and the README drift tests pass;
- CI runs on Linux, macOS and Windows.
