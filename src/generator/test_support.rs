//! Table and column builders for the generator's unit tests.

use crate::schema::{Column, Table};

/// A non-null, non-key, non-defaulted column whose Rust name and type
/// match its SQL name and the given type.
pub fn column(sql_name: &str, rust_type: &str) -> Column {
    Column {
        sql_name: sql_name.to_owned(),
        rust_name: sql_name.to_owned(),
        rust_type: rust_type.to_owned(),
        sqlx_type: rust_type.to_owned(),
        nullable: false,
        is_primary_key: false,
        has_default: false,
        is_generated: false,
        value_list: None,
        declared_length: None,
        value_range: None,
        length_range: None,
        typed_id: None,
        is_managed: false,
        id_generation: None,
    }
}

pub fn nullable(sql_name: &str, base_type: &str) -> Column {
    Column {
        rust_type: format!("Option<{base_type}>"),
        sqlx_type: base_type.to_owned(),
        nullable: true,
        ..column(sql_name, base_type)
    }
}

pub fn defaulted(mut column: Column) -> Column {
    column.has_default = true;
    column
}

pub fn generated(mut column: Column) -> Column {
    column.has_default = true;
    column.is_generated = true;
    column
}

/// A `users` table whose primary key is the listed columns (which are
/// flagged as key columns).
pub fn table(mut columns: Vec<Column>, primary_key: &[&str]) -> Table {
    for column in &mut columns {
        column.is_primary_key = primary_key.contains(&column.sql_name.as_str());
    }
    Table {
        schema: None,
        sql_name: "users".to_owned(),
        module_name: "users".to_owned(),
        struct_name: "User".to_owned(),
        columns,
        primary_key: primary_key.iter().map(|key| (*key).to_owned()).collect(),
        foreign_keys: Vec::new(),
        unique_keys: Vec::new(),
        key_constraints: Vec::new(),
        version_column: None,
        soft_delete_column: None,
        tenant_column: None,
        relationships: Vec::new(),
        before_insert_triggers: Vec::new(),
    }
}
