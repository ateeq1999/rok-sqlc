# 03: Migration directories, repeatable `--schema`, and `path:line` diagnostics

**What to build:** `--schema` accepts a file or a directory and can be repeated. A directory is read in filename order and `*.down.sql` files are skipped. Every table and query failure names the source file and the line of the offending statement (`migrations/0003_products.up.sql:12: table products: …`). The schema snapshot still holds the combined input verbatim.

**Blocked by:** 02

**Status:** ready-for-agent

- [ ] Running against a directory of numbered `*.up.sql`/`*.down.sql` files generates models from the up migrations only, applied in filename order (CLI seam).
- [ ] Repeating `--schema` combines inputs in the order given.
- [ ] An unsupported column type in the third migration of a directory produces an error that cites that file's path and the line of its `CREATE TABLE`.
- [ ] Query-file failures cite the query file's path and the annotation's line.
- [ ] Single-file inputs behave exactly as before.
