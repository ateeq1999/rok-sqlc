# Relationships: inferred once onto the schema model, loaded `with` loaders, read with `join`

ADR 0002 splits relationships into an inferred lane (single-hop relationships, from foreign keys) and a declared lane (what DDL cannot show). This ADR settles the inferred lane: which foreign keys become relationships, what they are called, where the generator records them, and the two ways generated code traverses them, **`with`** (a loader per relationship, for rows already fetched) and **`join`** (one joined read for a to-one relationship).

## Inference

Relationships are inferred **over the tables generated together**, after typed ids are resolved and conventions applied (ADRs 0010, 0013), by `generator::relationships::infer`. A foreign key makes a relationship when:

- it is not the **tenant column's** foreign key (every tenant table would otherwise sprout an `organization` relationship, and the tenant is a scope, not a navigation);
- the referenced table is in the run, matched exactly by schema and name like typed ids: a reference to a table that was filtered out, rejected, or lives in another migration set makes no relationship and degrades silently, so a module generated without its dependencies still generates;
- both columns have the same type, one a map key can hold (`Eq + Hash`: integers, text, UUIDs, decimals, dates and times, booleans). A foreign key whose column differs in type from the referenced one (`INTEGER` referencing `BIGINT`) makes none.

Each such foreign key gives two relationships:

- **Belongs-to** on the child, named after the column without `_id` (`category_id` → `category`). A column not ending in `_id` drops the referenced column's name instead (`currency_code` referencing `code` → `currency`); a column named like the referenced column (`id` referencing `id`) takes the referenced row's name (`user`); any other column keeps its name. A foreign key to a unique non-key column counts like one to the primary key.
- **Has-one** on the parent when the foreign key column alone is a unique key of the child (leaving out the tenant column, as unique-key lookups do), named after the child row (`user_profile`); otherwise **has-many**, named after the child module (`products`).
- A child with **several foreign keys to the same parent** names each inverse `{child}_by_{belongs-to}` (`invoices_by_created_by_user`, `invoices_by_approved_by_user`), so names are predictable from the columns.
- A **self-reference** through `parent_id` gives `parent` and `children` (`child` when unique). Any other self-reference uses the `_by_` form (`categories_by_parent_category`), since a bare `categories` on `categories` would say nothing.

Two relationships of one table with the same name **fail generation**, naming both (`relationships of \`users\` collide: \`profile\` names both the belongs-to … and the has-one …`). Renaming and opting out in configuration is how such a schema generates; the diagnostic says which foreign keys clash and points at `relationships.<table>`.

## Renaming and opting out

The `[relationships]` section (ADR 0011; `Config::relationships` in the library) is ADR 0002's declared lane limited to two edits of an inferred relationship, named by table (SQL, schema-qualified, or module name) and its **inferred** name:

```toml
[relationships.invoices]
created_by_user = "author"     # rename: load_author
approved_by_user = false       # opt out
```

- A **rename** changes only the name; it must be a snake_case Rust identifier. Everything generated from the relationship (loader, joined read, relationship filter) takes the new name, because renderers read the list. Each side is renamed on its own: renaming `invoices.created_by_user` leaves the inverse `users.invoices_by_created_by_user` as it is.
- An **opt-out** removes the relationship and its other side (the inverse of a belongs-to, the belongs-to of an inverse): half a foreign key's pair is never what a schema means. Renaming a side that another entry opts out is an error.
- `true` is an error, not a no-op.
- Entries are **checked against the whole schema**, inferred the same way, not just the tables a run generates: a `--resource` run accepts the configuration of a full run, and an entry for a table or relationship outside the run is skipped. An entry naming a table or relationship that does not exist fails, listing the table's relationships; a rename onto a name the table already uses fails the collision check, naming the entry (`` `relationships.invoices.created_by_user` ``).

Many-to-many through join tables builds on this model (ticket 23); declared has-many-through and polymorphic relationships remain ADR 0002's declared lane.

## The model

The result is recorded **on the schema model**, like conventions: `Table::relationships` is a list of `Relationship { name, kind, target, column, target_column }`, belongs-to first in foreign-key order, then the inverses in table order. `kind` is `BelongsTo`, `HasMany`, or `HasOne`; `target` is the related table's module name (unique in a run); `column` is the joining column on this table's rows (the foreign key for a belongs-to, the referenced column for an inverse) and `target_column` its partner on the related table. Renderers read this list and look the target up among the run's tables; none of them re-derive relationships from foreign keys. Later tickets plug in here:

- **Rename and opt-out** (ticket 24) edit the list between inference and the collision check: a rename changes `name`, an opt-out removes both sides of the foreign key's pair.
- **Many-to-many** (ticket 23) adds a kind whose target is the far table, recording the join table alongside.
- **Joined reads** (ticket 21) and **relationship filters** (ticket 22) iterate the to-one (resp. all) relationships of a table.

## `with`: loaders

Each resource with relationships gets **`relationships.rs`** with one `load_<name>` per relationship:

```rust
pub async fn load_category<'e, E>(executor: E, rows: &[Product])
    -> Result<HashMap<CategoryId, Category>, sqlx::Error>
pub async fn load_comments<'e, E>(executor: E, tenant_id: TenantId, rows: &[Post])
    -> Result<HashMap<PostId, Vec<Comment>>, sqlx::Error>
```

- It takes rows the caller already fetched and issues **one query** for all of them, `WHERE <target column> = ANY($n)`, binding the keys as an array (`sqlx(transparent)` typed ids have array support); no rows, no query.
- The map is keyed by the **joining key**, typed as the parent's column (its typed id where it has one): for a belongs-to the related row's key, which is the value of the child's foreign key; for an inverse the parent row's key. The caller looks up `loaded[&product.category_id]` or `loaded[&post.id]`, whichever side it holds. A primary key that is also a foreign key carries its own table's typed id; the loader converts it through the raw type.
- **To-one** (belongs-to, has-one) maps a key to one row; a key with no live related row is absent, and a `NULL` foreign key is skipped. **To-many** maps every input row's key to a list ordered by the related table's primary key, empty when nothing relates, so callers never special-case a missing entry.
- The query follows the **related table's** conventions: it takes the tenant right after the executor when that table has a tenant column and adds `<tenant> = $1`, and adds `<soft-delete column> IS NULL` when it has one. The loaded table, not the caller's, decides, because that is the table whose rows could leak.
- Like every generated function, it is generic over `PgExecutor` and runs one statement (ADR 0006).

Loaders return maps rather than attaching related rows to the caller's structs: generated rows stay plain `FromRow` structs, a caller loads only what a view needs, and composing two loaders is two lookups. "Nested" loading is loading the next relationship for the rows a loader returned.

## `join`: joined reads (ticket 21)

A to-one relationship also gets a single-query joined read, `get_with_<name>`, returning a struct holding both rows (the related row optional when the foreign key is nullable). Columns are aliased per side, since both tables commonly have `id` and `name`. The related table's tenant and soft-delete conditions go in the `JOIN … ON` clause, not `WHERE`, so a deleted parent yields `None` for the related row instead of hiding the child. Only to-one relationships are joined: a to-many join multiplies rows, and loaders already cover it in one more query.

## Relationship filters (ticket 22)

Each resource's search filter gains an optional field per relationship, named like it and holding the related resource's filter: `ProductFilter { category: Some(Box::new(CategoryFilter { name: vec![Text::Eq(…)], .. })), .. }`. Search does not join; each set field becomes a correlated subquery:

```sql
AND EXISTS (SELECT 1 FROM categories AS _r1 WHERE TRUE
    AND _r1.id = products.category_id
    [AND _r1.tenant_id = products.tenant_id] [AND deleted_at IS NULL]
    <the category filter's conditions, bare, resolving to _r1>)
```

- **`EXISTS`, never a join**, so a matching row comes back once however many related rows match. For a to-many relationship the field means "has at least one related row matching"; for a to-one, "its related row exists and matches". An **empty related filter** only requires a related row: a `NULL` foreign key, a soft-deleted parent, or an other-tenant row has none.
- The subquery's table is aliased by depth (`_r1`, `_r2`, …) and the outermost row is referenced by its table name, so self-references (`parent`, `children`) work. The searched table's own conditions stay bare.
- **Soft delete and tenancy follow the related table**, as for loaders. Soft-deleted related rows never match (`include_deleted` applies only to the searched table). A related tenant table is correlated on the tenant column with the enclosing row, which is itself bound to the tenant parameter at the root; a resource without a tenant column gets no field into a tenant table, since there is no tenant to scope it by.
- **Depth**: the filter types are recursive (`Box`), so nesting is limited at runtime to `db::search::MAX_RELATIONSHIP_DEPTH` (4) relationship hops; deeper filters make `search` fail with `sqlx::Error::InvalidArgument` instead of silently dropping conditions.
- A field exists only when the related resource has a filter that can nest (it has relationships of its own, as every target has the inverse). Filters of tables with relationships get a `push_conditions` function that other filters call.
- A relationship named like one of the filter's fields (a column's condition field, its `*_is_null` field, or `include_deleted`) **fails generation**, naming both; renaming it (ticket 24) resolves the clash.

## Considered Options

- **One query per row (lazy loading).** Rejected: the N+1 pattern this feature exists to remove.
- **`IN ($1, $2, …)` with one placeholder per key.** Rejected: the SQL text changes with the number of rows, so it cannot be static, and PostgreSQL caps parameters at 65 535. `= ANY($1)` binds one array.
- **Loaders that fill a field on the row (`product.category: Option<Category>`).** Rejected: rows would carry every relationship's type and an "unloaded" state that callers must check; maps keep rows plain.
- **Joining to-many relationships.** Rejected: duplicated parent rows and pagination that counts joined rows; loaders do it in two queries without either problem.
- **Inferring across the whole schema rather than the run.** Rejected: a relationship into a table that is not generated would reference a row type that does not exist.
- **Silently suffixing colliding names.** Rejected: names would depend on table order and change when a table is added; a diagnostic plus configuration (ticket 24) keeps names stable.
