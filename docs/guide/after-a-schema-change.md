# After a schema change

A schema change is a new migration, then `rok-sqlc g`. This page says what
that run rewrites, what it leaves to you, what can stop compiling and how
to fix it, and how CI catches a change nobody regenerated.

## The routine

1. Write a new migration (`migrations/0002_add_due_date.sql`). Never edit
   one that has been applied: the databases that ran it won't run it
   again.
2. Adjust `rok-sqlc.toml` if the change needs it: a new table's access,
   owner, hidden and search columns ([Configuring](configuring.md)), or an
   entry naming a column you dropped or renamed.
3. Run `rok-sqlc g`. It prints every file it `generated`, `scaffolded` or
   `removed`.
4. Build, and fix what the compiler points at (below).
5. Apply the migration to your database (`sqlx migrate run`), run the
   tests, and commit the migration and the generated code together.

## What regenerates and what stays yours

| Files | On a schema change |
|---|---|
| `src/db/**`, `src/shared/` (but `auth.rs` and `mod.rs`), `src/features/generated.rs`, `src/features/README.md` | rewritten |
| a feature's `mod.rs`, `routes.rs`, `handlers.rs`, `dto.rs`, `model.rs`, `repo.rs`, `service_api.rs` | rewritten |
| a feature's `service.rs` | never touched; scaffolded only for a new table |
| everything `init` scaffolded (`app.rs`, `config.rs`, `error.rs`, `openapi.rs`, ...) | never touched |
| your own files: migrations, query files, tests, hand-written features | never touched |

In detail, per kind of change:

- **A new table** gets a data-layer module, a feature folder with a
  scaffolded `service.rs`, and a line in `generated.rs`, which `app.rs`
  already calls. Its routes are live after the build, with `user` access
  unless you configure it.
- **A new column** appears in the row, the bodies (unless a convention,
  `owner` or the database leaves it out) and the response (unless hidden),
  and so in the document. A `NOT NULL` column without a default is a
  required field of the create body, so clients that don't send it get a
  `422`: add it with a default if existing clients must keep working.
- **A dropped or renamed column** leaves the row, the bodies and the
  response, and every `[api]` entry naming it fails the run until you
  change the entry.
- **A dropped table** loses its generated feature files and its data-layer
  module (`removed ...`). Its `service.rs` stays, because it is yours, but
  `generated.rs` no longer declares it, so it is no longer compiled: move
  what you need and delete the folder.
- **A renamed table** is a dropped table and a new one: the new name gets a
  fresh feature with a fresh `service.rs`. Move your overrides from the old
  folder's `service.rs` into the new one, rename the types they name, and
  delete the old folder. A `[modules]` entry keeps the old module name
  instead, if you'd rather keep the code and the routes as they were.

## What breaks, and how to fix it

The generated code and the defaults always agree with each other, so a
build failure after `rok-sqlc g` is in your code: an override, a port, a
hand-written feature or a test that names something the change altered.
The compiler points at each one.

| The change | What stops compiling | Fix |
|---|---|---|
| a column renamed or dropped | an override, port or test using the field (`input.due_on`, `row.due_on`) | use the new name, or drop the use |
| a column added | a test or override building a body or row as a struct literal (`CreateTaskDto { ... }`) | add the field; prefer a body's `into_new` or `..Default::default()` on update bodies where you can |
| a table gains `[api.owner]` or a tenant | overrides of its service methods, which now take `user: AuthUser` (or the tenant) | add the parameter where the new `service_api.rs` has it, and pass it on |
| a table gains a version column | `update_in` overrides, which now take the version after the key | add it, and handle `UpdateOutcome` as the default does |
| a key column's type changes | code naming the old typed id's inner type | use the new type; the typed id (`TaskId`) keeps its name |
| an endpoint goes (`[api.skip]`, a dropped key) | an override of its method, now not in the trait | delete the override |

An override's signature is the trait's: when in doubt, open the new
`service_api.rs` and copy the method's signature from there. A method you
never overrode needs nothing: its default is already up to date.

A query file whose SQL names a dropped column fails `rok-sqlc g` itself,
before it writes anything, naming the query and the line:

```text
error: queries.sql:2: query `Bad`: column `nope` does not exist in `articles`
Error: 1 unsupported query in `queries.sql`; pass --lenient to skip them
```

Fix the SQL and run it again.

## In CI: `--check`

```sh
rok-sqlc g --check
```

`--check` writes nothing. It regenerates in memory and lists every file
that differs from what is committed, then exits 1. After a migration
adding a column to `articles`, committed without regenerating:

```text
changed src/db/users/relationships.rs
changed src/db/articles/listing.rs
changed src/db/articles/models.rs
changed src/db/articles/filter.rs
changed src/db/articles/read.rs
changed src/db/articles/relationships.rs
changed src/db/articles/write.rs
changed src/features/articles/dto.rs
changed src/db/schema.rs
Error: generated code is out of date: 9 file(s) drifted; run `rok-sqlc generate` to update them
```

(`missing` for files a run would create, `changed` for files it would
rewrite, `stale` for generated files it would remove.) A pull request that
adds a migration without regenerating, or edits a generated file by hand,
fails it. A scaffolded file only counts when it is missing, never for its
contents, so your edits to `service.rs` never fail the check. Line endings
don't count either, so a Windows checkout with CRLF passes.

Run it with the same rok-sqlc version the project generates with; a newer
version may generate different code ([Deploying](deploying.md#checks-in-ci)).
