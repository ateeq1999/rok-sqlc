//! `ALTER TABLE … DROP CONSTRAINT` and named `ADD CONSTRAINT … CHECK`.

use super::*;

const AUTH_TOKENS: &str = "CREATE TABLE auth_tokens (
    id UUID PRIMARY KEY,
    purpose TEXT NOT NULL CHECK (
        purpose IN ('email_verification', 'password_reset')
    )
);";

#[test]
fn a_value_list_check_replaced_by_name_widens_its_enum() {
    let parsed = parse_files(&[
        ("0001_auth.sql", AUTH_TOKENS),
        (
            "0002_widen.sql",
            "ALTER TABLE auth_tokens DROP CONSTRAINT auth_tokens_purpose_check;
             ALTER TABLE auth_tokens ADD CONSTRAINT auth_tokens_purpose_check CHECK (
                 purpose IN ('email_verification', 'password_reset', 'two_factor_pending')
             );",
        ),
    ]);
    assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
    assert_eq!(
        enum_summary(&parsed.schema),
        [(
            "AuthTokenPurpose".to_owned(),
            Some("auth_tokens.purpose"),
            vec!["email_verification", "password_reset", "two_factor_pending"]
        )]
    );
}

#[test]
fn dropping_an_implicitly_named_unique_constraint_drops_its_unique_key() {
    let table = one_table(
        "CREATE TABLE users (id BIGSERIAL PRIMARY KEY, email TEXT NOT NULL UNIQUE);
         ALTER TABLE users DROP CONSTRAINT users_email_key;",
    );
    assert!(table.unique_keys.is_empty(), "{:?}", table.unique_keys);
    assert_eq!(table.key_constraints.len(), 1);
    assert_eq!(table.key_constraints[0].name, "users_pkey");
}

#[test]
fn dropping_an_implicitly_named_foreign_key_drops_it() {
    let parsed = parse(
        "CREATE TABLE users (id BIGSERIAL PRIMARY KEY);
         CREATE TABLE posts (
             id BIGSERIAL PRIMARY KEY,
             author_id BIGINT REFERENCES users (id),
             editor_id BIGINT,
             FOREIGN KEY (editor_id) REFERENCES users (id)
         );
         ALTER TABLE posts DROP CONSTRAINT posts_author_id_fkey;",
    );
    assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
    let posts = &parsed.schema.tables[1];
    let names: Vec<(&str, &str)> = posts
        .foreign_keys
        .iter()
        .map(|key| (key.name.as_str(), key.column.as_str()))
        .collect();
    assert_eq!(names, [("posts_editor_id_fkey", "editor_id")]);
}

#[test]
fn dropping_an_unknown_constraint_warns_and_keeps_the_table_unless_if_exists() {
    let parsed = parse_files(&[
        (
            "0001.sql",
            "CREATE TABLE notes (id BIGSERIAL PRIMARY KEY, body TEXT);
             CREATE TABLE tags (id BIGSERIAL PRIMARY KEY);",
        ),
        (
            "0002.sql",
            "ALTER TABLE tags DROP CONSTRAINT IF EXISTS tags_missing;
             ALTER TABLE notes DROP CONSTRAINT notes_no_overlap;
             ALTER TABLE notes DROP CONSTRAINT notes_id_not_null;",
        ),
    ]);
    assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
    assert_eq!(parsed.schema.tables.len(), 2);
    assert_eq!(
        parsed.warnings,
        [
            "0002.sql:2: table `notes`: DROP CONSTRAINT: constraint `notes_no_overlap` is not one rok-sqlc tracks (such as an EXCLUDE or NOT NULL constraint); the statement changes nothing in the generated code",
            "0002.sql:3: table `notes`: DROP CONSTRAINT: constraint `notes_id_not_null` is not one rok-sqlc tracks (such as an EXCLUDE or NOT NULL constraint); the statement changes nothing in the generated code",
        ]
    );
}

fn named_constraints(table: &Table) -> Vec<&str> {
    table
        .named_constraints
        .iter()
        .map(|named| named.name.as_str())
        .collect()
}

#[test]
fn checks_get_postgres_default_names() {
    let table = one_table(
        "CREATE TABLE prices (
             id BIGSERIAL PRIMARY KEY,
             low INTEGER CHECK (low >= 0),
             high INTEGER CHECK (high >= 0) CHECK (high < 1000),
             CHECK (low <= high),
             CONSTRAINT sane CHECK (true)
         );
         ALTER TABLE prices ADD CHECK (low < 500);
         ALTER TABLE prices ADD COLUMN note TEXT CHECK (note <> '');",
    );
    assert_eq!(
        named_constraints(&table),
        [
            "prices_low_check",
            "prices_high_check",
            "prices_high_check1",
            "prices_check",
            "sane",
            "prices_low_check1",
            "prices_note_check",
        ]
    );
}

#[test]
fn dropping_one_of_two_checks_keeps_what_the_other_says() {
    let table = one_table(
        "CREATE TABLE items (
             id BIGSERIAL PRIMARY KEY,
             qty INTEGER CHECK (qty >= 1) CHECK (qty <= 9),
             kind TEXT CHECK (kind IN ('a', 'b', 'c')),
             CONSTRAINT kind_narrow CHECK (kind IN ('a', 'b'))
         );
         ALTER TABLE items DROP CONSTRAINT items_qty_check1;
         ALTER TABLE items DROP CONSTRAINT kind_narrow;",
    );
    let qty = table.columns[1].value_range.as_ref().expect("a range");
    assert_eq!(
        qty.min.as_ref().map(|bound| bound.value.as_str()),
        Some("1")
    );
    assert_eq!(qty.max, None);
    assert_eq!(
        table.columns[2].value_list.as_deref(),
        Some(&["a".to_owned(), "b".to_owned(), "c".to_owned()][..])
    );
}

#[test]
fn a_dropped_value_list_check_leaves_plain_text() {
    let parsed = parse(&format!(
        "{AUTH_TOKENS}
         ALTER TABLE auth_tokens DROP CONSTRAINT auth_tokens_purpose_check;"
    ));
    assert!(parsed.schema.enums.is_empty());
    assert_eq!(parsed.schema.tables[0].columns[1].rust_type, "String");
}

#[test]
fn a_check_follows_its_column_through_a_rename() {
    let table = one_table(&format!(
        "{AUTH_TOKENS}
         ALTER TABLE auth_tokens RENAME COLUMN purpose TO kind;
         ALTER TABLE auth_tokens ADD CONSTRAINT kind_short CHECK (char_length(kind) <= 40);
         ALTER TABLE auth_tokens DROP CONSTRAINT kind_short;"
    ));
    assert_eq!(
        table.columns[1].value_list.as_deref(),
        Some(&["email_verification".to_owned(), "password_reset".to_owned()][..])
    );
    assert_eq!(table.named_constraints[0].columns, ["kind"]);
}

#[test]
fn dropping_the_primary_key_leaves_its_columns_required() {
    let table = one_table(
        "CREATE TABLE tags (code TEXT PRIMARY KEY, label TEXT);
         ALTER TABLE tags DROP CONSTRAINT tags_pkey;
         ALTER TABLE tags ADD CONSTRAINT tags_label_pk PRIMARY KEY (label);",
    );
    assert_eq!(table.primary_key, ["label"]);
    assert!(!table.columns[0].is_primary_key);
    assert!(!table.columns[0].nullable);
    assert!(table.columns[1].is_primary_key);
}

#[test]
fn dropping_a_column_drops_its_checks_and_renamed_constraints_keep_working() {
    let table = one_table(
        "CREATE TABLE notes (id BIGSERIAL PRIMARY KEY, body TEXT CHECK (body <> ''), flag TEXT);
         ALTER TABLE notes DROP COLUMN body;
         ALTER TABLE notes ADD CONSTRAINT flag_values CHECK (flag IN ('x', 'y'));
         ALTER TABLE notes RENAME CONSTRAINT flag_values TO notes_flag_values;
         ALTER TABLE notes DROP CONSTRAINT notes_flag_values;",
    );
    assert!(table.named_constraints.is_empty());
    assert_eq!(table.columns[1].value_list, None);
}

#[test]
fn a_composite_foreign_key_is_kept_by_name_and_can_be_dropped() {
    let parsed = parse(
        "CREATE TABLE p2 (x BIGINT, y BIGINT, UNIQUE (x, y));
         CREATE TABLE posts (a BIGINT, b BIGINT, FOREIGN KEY (a, b) REFERENCES p2 (x, y));",
    );
    assert_eq!(
        named_constraints(&parsed.schema.tables[1]),
        ["posts_a_b_fkey"]
    );
    let table = one_table(
        "CREATE TABLE posts (a BIGINT, b BIGINT, FOREIGN KEY (a, b) REFERENCES p2 (x, y));
         ALTER TABLE posts DROP CONSTRAINT posts_a_b_fkey;",
    );
    assert!(table.named_constraints.is_empty());
}
