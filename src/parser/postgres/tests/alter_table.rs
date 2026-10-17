use super::*;
use crate::schema::UniqueKey;

#[test]
fn applies_column_changes_from_later_migrations() {
    let parsed = parse_files(&[
        (
            "0001_create.up.sql",
            "CREATE TABLE notes (id BIGSERIAL PRIMARY KEY, title TEXT NOT NULL, legacy TEXT);",
        ),
        (
            "0002_alter.up.sql",
            "ALTER TABLE notes ADD COLUMN body TEXT NOT NULL DEFAULT '';
                 ALTER TABLE notes RENAME COLUMN title TO headline;
                 ALTER TABLE notes DROP COLUMN legacy;",
        ),
    ]);
    assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
    let table = &parsed.schema.tables[0];
    assert_eq!(column_names(table), ["id", "headline", "body"]);
    assert_eq!(table.columns[1].rust_name, "headline");
    assert_eq!(table.columns[1].rust_type, "String");
    assert_eq!(table.columns[2].rust_type, "String");
    assert!(table.columns[2].has_default);
}

#[test]
fn records_constraints_added_by_alter_table_like_inline_ones() {
    let table = one_table(
        "CREATE TABLE app.posts (id UUID NOT NULL, author_id UUID, slug TEXT NOT NULL);
             ALTER TABLE app.posts ADD CONSTRAINT posts_pk PRIMARY KEY (id);
             ALTER TABLE app.posts ADD CONSTRAINT posts_author_fk
               FOREIGN KEY (author_id) REFERENCES app.users (id);
             ALTER TABLE app.posts ADD CONSTRAINT posts_slug_key UNIQUE (slug);",
    );
    assert_eq!(table.primary_key, ["id"]);
    assert!(table.columns[0].is_primary_key);
    assert_eq!(table.foreign_keys.len(), 1);
    assert_eq!(table.foreign_keys[0].column, "author_id");
    assert_eq!(
        table.foreign_keys[0].referenced_schema.as_deref(),
        Some("app")
    );
    assert_eq!(table.foreign_keys[0].referenced_table, "users");
    assert_eq!(table.foreign_keys[0].referenced_column, "id");
    assert_eq!(table.unique_keys, [UniqueKey::new(vec!["slug".to_owned()])]);
}

#[test]
fn primary_key_added_later_makes_its_column_required() {
    let table = one_table(
        "CREATE TABLE tags (code TEXT, label TEXT);
             ALTER TABLE tags ADD PRIMARY KEY (code);",
    );
    assert!(!table.columns[0].nullable);
    assert_eq!(table.columns[0].rust_type, "String");
    assert_eq!(table.columns[1].rust_type, "Option<String>");
}

#[test]
fn renaming_a_key_column_renames_it_in_every_key() {
    let table = one_table(
            "CREATE TABLE posts (id BIGSERIAL PRIMARY KEY, author_id UUID REFERENCES users (id) UNIQUE);
             ALTER TABLE posts RENAME COLUMN id TO post_id;
             ALTER TABLE posts RENAME COLUMN author_id TO writer_id;",
        );
    assert_eq!(table.primary_key, ["post_id"]);
    assert_eq!(table.foreign_keys[0].column, "writer_id");
    assert_eq!(table.unique_keys[0].columns, ["writer_id"]);
}

#[test]
fn dropping_a_key_column_drops_its_keys() {
    let table = one_table(
            "CREATE TABLE posts (id BIGINT PRIMARY KEY, author_id UUID REFERENCES users (id), title TEXT UNIQUE);
             ALTER TABLE posts DROP COLUMN id, DROP COLUMN author_id, DROP COLUMN title, ADD COLUMN body TEXT;",
        );
    assert!(table.primary_key.is_empty());
    assert!(table.foreign_keys.is_empty());
    assert!(table.unique_keys.is_empty());
    assert_eq!(column_names(&table), ["body"]);
}

#[test]
fn ignores_statements_that_leave_the_model_unchanged() {
    let parsed = parse(
            "CREATE TABLE notes (id BIGSERIAL PRIMARY KEY, body TEXT);
             ALTER TABLE notes ENABLE ROW LEVEL SECURITY;
             CREATE POLICY notes_tenant ON notes USING (true);
             CREATE INDEX notes_body_index ON notes (body);
             GRANT SELECT ON notes TO reader;
             CREATE FUNCTION touch() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$;
             CREATE TRIGGER notes_touch BEFORE UPDATE ON notes FOR EACH ROW EXECUTE FUNCTION touch();
             COMMENT ON TABLE notes IS 'Notes';
             SELECT prepare_table('notes');",
        );
    assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
    let table = &parsed.schema.tables[0];
    assert_eq!(column_names(table), ["id", "body"]);
    assert!(table.unique_keys.is_empty());
}

#[test]
fn rejects_unsupported_alter_table_on_a_known_table_with_its_location() {
    let parsed = parse_files(&[
        (
            "migrations/0001_create.up.sql",
            "CREATE TABLE prices (id BIGSERIAL PRIMARY KEY, amount INTEGER NOT NULL);",
        ),
        (
            "migrations/0002_rename.up.sql",
            "-- rename prices\n\nALTER TABLE prices RENAME TO costs;",
        ),
    ]);
    assert!(parsed.schema.tables.is_empty());
    assert_eq!(parsed.failures.len(), 1);
    let message = parsed.failures[0].to_string();
    assert!(
        message.starts_with(&format!(
            "{}:3: table `prices`: unsupported ALTER TABLE clause `RENAME",
            Path::new("migrations/0002_rename.up.sql").display()
        )),
        "{message}"
    );
}

/// Like a `CHECK` in `CREATE TABLE`, one added later changes the model
/// only when it is a value list on a text column.
#[test]
fn ignores_checks_added_later_that_are_not_value_lists() {
    let parsed = parse(
        "CREATE TABLE prices (id BIGSERIAL PRIMARY KEY, amount INTEGER NOT NULL, code TEXT);
             ALTER TABLE prices ADD CONSTRAINT positive CHECK (amount > 0);
             ALTER TABLE prices ADD CHECK (amount IN (1, 2)), ADD CHECK (code <> '');
             ALTER TABLE prices ADD CHECK (id IN ('1'));",
    );
    assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
    let table = &parsed.schema.tables[0];
    assert!(
        table
            .columns
            .iter()
            .all(|column| column.value_list.is_none()),
        "{:?}",
        table.columns
    );
    assert!(parsed.schema.enums.is_empty());
}

#[test]
fn rejects_alter_table_naming_a_missing_column() {
    let parsed = parse(
        "CREATE TABLE notes (id BIGSERIAL PRIMARY KEY);
             ALTER TABLE notes RENAME COLUMN title TO headline;",
    );
    assert!(parsed.schema.tables.is_empty());
    assert!(
        parsed.failures[0]
            .reason
            .contains("RENAME COLUMN: column `title` does not exist")
    );
}

#[test]
fn ignores_alter_table_on_unknown_tables() {
    let table = one_table(
        "CREATE TABLE notes (id BIGSERIAL PRIMARY KEY);
             ALTER TABLE other.notes ALTER COLUMN id TYPE BIGINT;
             ALTER TABLE elsewhere ADD COLUMN x POINT;",
    );
    assert_eq!(column_names(&table), ["id"]);
}

#[test]
fn rejects_columns_added_with_unsupported_types() {
    let parsed = parse(
        "CREATE TABLE shifts (id BIGSERIAL PRIMARY KEY);
             ALTER TABLE public.shifts ADD COLUMN span POINT;",
    );
    assert!(parsed.schema.tables.is_empty());
    assert_eq!(parsed.failures[0].line, Some(2));
    assert!(
        parsed.failures[0]
            .reason
            .contains("ADD COLUMN: column `span`: type `POINT` is not supported yet"),
        "{:?}",
        parsed.failures
    );
}

/// Expected names are the ones PostgreSQL 17 chose for the same DDL.
#[test]
fn key_constraints_get_postgres_default_names() {
    let names = |sql: &str| -> Vec<String> {
        one_table(sql)
            .key_constraints
            .into_iter()
            .map(|constraint| constraint.name)
            .collect()
    };
    assert_eq!(
        names(
            "CREATE TABLE t (id BIGINT PRIMARY KEY, a TEXT UNIQUE, b TEXT, c TEXT, d TEXT,
                     CONSTRAINT named_bc UNIQUE (b, c));
                 ALTER TABLE t ADD UNIQUE (c, d);
                 CREATE UNIQUE INDEX t_d_idx ON t (d);"
        ),
        ["t_pkey", "t_a_key", "named_bc", "t_c_d_key"]
    );
    assert_eq!(
        names(
            "CREATE TABLE averyveryveryveryveryverylongtablenamethatgoesonandonandon (
                     abcdefghijklmnopqrstuvwxyzabcdefghij TEXT UNIQUE, x TEXT)"
        ),
        ["averyveryveryveryveryverylong_abcdefghijklmnopqrstuvwxyzabc_key"]
    );
    assert_eq!(
        names("CREATE TABLE u (a TEXT UNIQUE, b TEXT, UNIQUE (a)); ALTER TABLE u ADD UNIQUE (a)"),
        ["u_a_key", "u_a_key1"]
    );
}

#[test]
fn records_unique_indexes_on_plain_columns() {
    let table = one_table(
        "CREATE TABLE users (id BIGSERIAL PRIMARY KEY, email TEXT, deleted_at TIMESTAMPTZ);
             CREATE UNIQUE INDEX users_email_live ON users (email) WHERE deleted_at IS NULL;
             CREATE UNIQUE INDEX users_email_lower ON users (lower(email));
             CREATE INDEX users_deleted ON users (deleted_at);",
    );
    assert_eq!(
        table.unique_keys,
        [UniqueKey {
            columns: vec!["email".to_owned()],
            predicate: Some("deleted_at IS NULL".to_owned()),
        }]
    );
}
