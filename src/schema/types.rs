#[derive(Debug, Clone)]
pub struct Schema {
    pub tables: Vec<Table>,
}

#[derive(Debug, Clone)]
pub struct Table {
    /// The schema qualifier as written in the DDL (`catalog` in
    /// `catalog.products`); `None` when the table name is unqualified.
    pub schema: Option<String>,
    pub sql_name: String,
    pub module_name: String,
    pub struct_name: String,
    pub columns: Vec<Column>,
    /// Primary key columns in declaration order; empty when the table has no primary key.
    pub primary_key: Vec<String>,
    pub foreign_keys: Vec<ForeignKey>,
}

impl Table {
    /// The table name as a human reads it in DDL, e.g. `catalog.products`.
    pub fn display_name(&self) -> String {
        match &self.schema {
            Some(schema) => format!("{schema}.{}", self.sql_name),
            None => self.sql_name.clone(),
        }
    }

    /// Primary key columns in key declaration order.
    pub fn primary_key_columns(&self) -> Vec<&Column> {
        self.primary_key
            .iter()
            .filter_map(|key| self.columns.iter().find(|column| column.sql_name == *key))
            .collect()
    }

    /// Columns a caller may supply on insert.
    pub fn insert_columns(&self) -> Vec<&Column> {
        self.columns
            .iter()
            .filter(|column| column.is_insertable())
            .collect()
    }

    /// Columns a caller may change on update.
    pub fn update_columns(&self) -> Vec<&Column> {
        self.columns
            .iter()
            .filter(|column| column.is_updatable())
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct Column {
    pub sql_name: String,
    pub rust_name: String,
    pub rust_type: String,
    pub sqlx_type: String,
    pub nullable: bool,
    pub is_primary_key: bool,
    /// The database can supply a value (DEFAULT clause, serial, or identity).
    pub has_default: bool,
    /// The database always owns the value (serial, identity, or GENERATED);
    /// the column is never inserted or updated by generated code.
    pub is_generated: bool,
}

impl Column {
    /// The caller may supply this column on insert.
    pub fn is_insertable(&self) -> bool {
        !self.is_generated
    }

    /// The caller may omit this column on insert so the database default applies.
    pub fn is_omittable_on_insert(&self) -> bool {
        self.is_insertable() && self.has_default
    }

    /// The caller may change this column on update: it is neither generated
    /// nor part of the primary key.
    pub fn is_updatable(&self) -> bool {
        !self.is_generated && !self.is_primary_key
    }
}

#[derive(Debug, Clone)]
pub struct ForeignKey {
    pub column: String,
    pub referenced_table: String,
    pub referenced_column: String,
}
