//! The relation names a schema takes, the names PostgreSQL gives unnamed
//! constraints, and identifier helpers.

use std::collections::{HashMap, HashSet};

use sqlparser::ast::{
    ColumnDef, CreateIndex, Expr, Ident, IndexColumn, ObjectName, ObjectNamePart,
};

use super::types::is_serial_type;
use crate::schema::Table;

/// `public` is on every default search path, so `public.x` is `x`.
pub(super) fn normalize_schema(schema: Option<String>) -> Option<String> {
    schema.filter(|schema| schema != "public")
}

/// The known table an `ALTER TABLE` or `CREATE INDEX` names. An unqualified
/// name and a `public.` one refer to the same table.
pub(super) fn find_table(tables: &[Table], name: &ObjectName) -> Option<usize> {
    let leaf = object_leaf_name(name);
    let schema = object_schema_name(name);
    tables.iter().position(|table| {
        table.sql_name == leaf && same_schema(table.schema.as_deref(), schema.as_deref())
    })
}

/// Whether two schema names are one schema: no schema is `public`.
fn same_schema(left: Option<&str>, right: Option<&str>) -> bool {
    fn normalize(schema: Option<&str>) -> Option<&str> {
        schema.filter(|schema| *schema != "public")
    }
    normalize(left) == normalize(right)
}

/// The relation names taken in `schema`: each known table's name and its key
/// constraints' (whose indexes are relations), and the `others`. PostgreSQL
/// gives an unnamed constraint a name no relation in its schema has.
pub(super) fn relation_names(
    tables: &[Table],
    others: &OtherRelations,
    schema: Option<&str>,
) -> HashSet<String> {
    tables
        .iter()
        .filter(|table| same_schema(table.schema.as_deref(), schema))
        .flat_map(|table| {
            std::iter::once(table.sql_name.clone()).chain(
                table
                    .key_constraints
                    .iter()
                    .map(|constraint| constraint.name.clone()),
            )
        })
        .chain(others.in_schema(schema).cloned())
        .collect()
}

/// Applies a rename of an index, sequence, or view (`ALTER INDEX … RENAME
/// TO`, and the like) in the schema `name` is in: the old name is free and
/// the new one taken. The index behind a primary key or unique constraint
/// shares the constraint's name, so renaming it renames the constraint, as
/// in PostgreSQL. An unknown name changes nothing, like `IF EXISTS`.
pub(super) fn rename_relation(
    tables: &mut [Table],
    others: &mut OtherRelations,
    name: &ObjectName,
    new_name: &str,
) {
    let schema = object_schema_name(name);
    let leaf = object_leaf_name(name);
    if others.remove(schema.as_deref(), &leaf) {
        others.insert(schema.as_deref(), new_name.to_owned());
        return;
    }
    let constraint = tables
        .iter_mut()
        .filter(|table| same_schema(table.schema.as_deref(), schema.as_deref()))
        .flat_map(|table| table.key_constraints.iter_mut())
        .find(|constraint| constraint.name == leaf);
    if let Some(constraint) = constraint {
        new_name.clone_into(&mut constraint.name);
    }
}

/// Relations that take a name in their schema besides the known tables and
/// their key constraints: indexes made by `CREATE INDEX` (named, or under
/// PostgreSQL's default `<table>_<columns>_idx`), sequences (`CREATE
/// SEQUENCE`, and a serial column's `<table>_<column>_seq`), and views and
/// materialized views, less any dropped since, under their latest names
/// (see [`rename_relation`]).
#[derive(Default)]
pub(super) struct OtherRelations {
    /// Names by schema (`None` is `public`).
    names: HashMap<Option<String>, HashSet<String>>,
}

impl OtherRelations {
    fn in_schema(&self, schema: Option<&str>) -> impl Iterator<Item = &String> {
        self.names
            .get(&normalize_schema(schema.map(str::to_owned)))
            .into_iter()
            .flatten()
    }

    fn insert(&mut self, schema: Option<&str>, name: String) {
        self.names
            .entry(normalize_schema(schema.map(str::to_owned)))
            .or_default()
            .insert(name);
    }

    /// Records the relation `name` (schema-qualified or not) creates.
    pub(super) fn create(&mut self, name: &ObjectName) {
        self.insert(object_schema_name(name).as_deref(), object_leaf_name(name));
    }

    /// Forgets a dropped relation.
    pub(super) fn drop(&mut self, name: &ObjectName) {
        self.remove(object_schema_name(name).as_deref(), &object_leaf_name(name));
    }

    /// Forgets `name` in `schema`; whether it was there.
    fn remove(&mut self, schema: Option<&str>, name: &str) -> bool {
        self.names
            .get_mut(&normalize_schema(schema.map(str::to_owned)))
            .is_some_and(|names| names.remove(name))
    }

    /// Records the sequences of the serial columns among `columns` that
    /// `table` gets, each under PostgreSQL's default name, and adds them to
    /// `taken`.
    pub(super) fn create_sequences<'a>(
        &mut self,
        table: &ObjectName,
        columns: impl IntoIterator<Item = &'a ColumnDef>,
        taken: &mut HashSet<String>,
    ) {
        let schema = object_schema_name(table);
        let table = object_leaf_name(table);
        for column in columns {
            if !is_serial_type(&column.data_type) {
                continue;
            }
            let column = ident_name(&column.name);
            let name =
                choose_relation_name(&table, Some(&column), "seq", |name| taken.contains(name));
            taken.insert(name.clone());
            self.insert(schema.as_deref(), name);
        }
    }

    /// Records the index a `CREATE INDEX` makes: under its name, or under
    /// PostgreSQL's default one, which avoids the names `taken` in its schema.
    pub(super) fn create_index(&mut self, index: &CreateIndex, taken: &HashSet<String>) {
        let schema = object_schema_name(&index.table_name);
        let name = match &index.name {
            Some(name) => object_leaf_name(name),
            None => {
                let columns: Vec<String> = index
                    .columns
                    .iter()
                    .map(|column| index_column_name(column).unwrap_or_else(|| "expr".to_owned()))
                    .collect();
                choose_relation_name(
                    &object_leaf_name(&index.table_name),
                    Some(&columns.join("_")),
                    "idx",
                    |name| taken.contains(name),
                )
            }
        };
        self.insert(schema.as_deref(), name);
    }
}

/// PostgreSQL's longest identifier, in bytes (`NAMEDATALEN - 1`).
const MAX_IDENTIFIER_BYTES: usize = 63;

/// The name PostgreSQL gives an unnamed primary key (`<table>_pkey`) or
/// unique constraint (`<table>_<col>_<col>_key`); see
/// [`choose_relation_name`].
pub(super) fn default_constraint_name(
    table: &str,
    columns: &[String],
    primary: bool,
    taken: impl Fn(&str) -> bool,
) -> String {
    if primary {
        choose_relation_name(table, None, "pkey", taken)
    } else {
        choose_relation_name(table, Some(&columns.join("_")), "key", taken)
    }
}

/// PostgreSQL's `ChooseRelationName`: `name1[_name2]_label`, with the parts
/// shortened, the longer one first, until the name fits in 63 bytes, and a
/// number after the label (`_key1`) while the name is `taken`. PostgreSQL
/// avoids every relation name in the schema; `taken` says which are.
fn choose_relation_name(
    name1: &str,
    name2: Option<&str>,
    label: &str,
    taken: impl Fn(&str) -> bool,
) -> String {
    let mut pass = 0;
    loop {
        let label = if pass == 0 {
            label.to_owned()
        } else {
            format!("{label}{pass}")
        };
        let name = object_name(name1, name2, &label);
        if !taken(&name) {
            return name;
        }
        pass += 1;
    }
}

/// PostgreSQL's `makeObjectName`: `name1[_name2]_label`, with `name1` and
/// `name2` shortened (the longer one first, on character boundaries) so the
/// whole fits in [`MAX_IDENTIFIER_BYTES`].
fn object_name(name1: &str, name2: Option<&str>, label: &str) -> String {
    let overhead = label.len() + 1 + usize::from(name2.is_some());
    let available = MAX_IDENTIFIER_BYTES.saturating_sub(overhead);
    let (mut len1, mut len2) = (name1.len(), name2.map_or(0, str::len));
    while len1 + len2 > available {
        if len1 > len2 {
            len1 -= 1;
        } else {
            len2 -= 1;
        }
    }
    let clip = |name: &str, mut len: usize| {
        while !name.is_char_boundary(len) {
            len -= 1;
        }
        name[..len].to_owned()
    };
    let mut name = clip(name1, len1);
    if let Some(name2) = name2 {
        name.push('_');
        name.push_str(&clip(name2, len2));
    }
    name.push('_');
    name.push_str(label);
    name
}

pub(crate) fn ident_name(ident: &Ident) -> String {
    match ident.quote_style {
        Some('"') => ident.value.clone(),
        _ => ident.value.to_lowercase(),
    }
}

pub(crate) fn object_leaf_name(name: &ObjectName) -> String {
    name.0
        .iter()
        .rev()
        .find_map(|part| match part {
            ObjectNamePart::Identifier(ident) => Some(ident_name(ident)),
            ObjectNamePart::Function(_) => None,
        })
        .unwrap_or_default()
}

/// The schema qualifier of a possibly qualified name (`catalog` in
/// `catalog.products`); `None` for an unqualified name.
pub(super) fn object_schema_name(name: &ObjectName) -> Option<String> {
    let identifiers: Vec<&Ident> = name
        .0
        .iter()
        .filter_map(|part| match part {
            ObjectNamePart::Identifier(ident) => Some(ident),
            ObjectNamePart::Function(_) => None,
        })
        .collect();
    match identifiers.as_slice() {
        [.., schema, _table] => Some(ident_name(schema)),
        _ => None,
    }
}

pub(super) fn index_column_name(column: &IndexColumn) -> Option<String> {
    match &column.column.expr {
        Expr::Identifier(ident) => Some(ident_name(ident)),
        _ => None,
    }
}
