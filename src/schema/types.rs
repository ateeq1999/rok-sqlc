#[derive(Debug, Clone)]
pub struct Schema {
    pub tables: Vec<Table>,
}

#[derive(Debug, Clone)]
pub struct Table {
    pub sql_name: String,
    pub module_name: String,
    pub struct_name: String,
    pub columns: Vec<Column>,
    /// Primary key columns in declaration order; empty when the table has no primary key.
    pub primary_key: Vec<String>,
    pub foreign_keys: Vec<ForeignKey>,
}

#[derive(Debug, Clone)]
pub struct Column {
    pub sql_name: String,
    pub rust_name: String,
    pub rust_type: String,
    pub sqlx_type: String,
    pub nullable: bool,
    pub is_primary_key: bool,
}

#[derive(Debug, Clone)]
pub struct ForeignKey {
    pub column: String,
    pub referenced_table: String,
    pub referenced_column: String,
}
