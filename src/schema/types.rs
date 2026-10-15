use heck::ToUpperCamelCase;

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
    /// Unique constraints and unique indexes on plain columns, in the order
    /// the migrations declare them; the primary key is not repeated here.
    pub unique_keys: Vec<UniqueKey>,
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
    /// The typed id the generated code uses in place of `sqlx_type`: the
    /// table's own id for a single-column primary key, or the referenced
    /// table's id for a foreign key to it. The parser leaves it `None`; the
    /// generator fills it in for the tables it generates.
    pub typed_id: Option<TypedId>,
}

/// A generated typed id: `name` is defined in `<module>/models.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedId {
    pub module: String,
    pub name: String,
}

/// How a column can be filtered by the generated `search` API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    /// Numbers, decimals, dates, and times: comparisons, ranges, and sets.
    Ordered,
    /// Text: equality, sets, and literal substring matching.
    Text,
    /// Booleans and UUIDs: equality and sets.
    Equality,
}

impl Column {
    /// How the column can be filtered, or `None` for arrays and JSON, which
    /// get only a null check.
    pub fn filter_kind(&self) -> Option<FilterKind> {
        match self.sqlx_type.as_str() {
            "i16"
            | "i32"
            | "i64"
            | "f32"
            | "f64"
            | "rust_decimal::Decimal"
            | "chrono::NaiveDate"
            | "chrono::NaiveTime"
            | "chrono::NaiveDateTime"
            | "chrono::DateTime<chrono::Utc>" => Some(FilterKind::Ordered),
            "String" => Some(FilterKind::Text),
            "bool" | "uuid::Uuid" => Some(FilterKind::Equality),
            _ => None,
        }
    }

    /// The `XSortKey` variant naming this column.
    pub fn sort_variant(&self) -> String {
        let camel = self.rust_name.trim_matches('_').to_upper_camel_case();
        if camel.starts_with(|character: char| character.is_ascii_digit()) {
            format!("Column{camel}")
        } else {
            camel
        }
    }

    /// The `XFilter` field selecting rows where this column is or is not
    /// `NULL` (`note_is_null`; `type_` gives `type_is_null`).
    pub fn null_check_field(&self) -> String {
        format!("{}_is_null", self.rust_name.trim_end_matches('_'))
    }

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

    /// The Rust type of one non-null value: the typed id when the column has
    /// one, otherwise `sqlx_type`.
    pub fn value_type(&self) -> &str {
        match &self.typed_id {
            Some(id) => &id.name,
            None => &self.sqlx_type,
        }
    }

    /// The Rust type of the column's field: `value_type`, wrapped in
    /// `Option` when the column is nullable.
    pub fn field_type(&self) -> String {
        if self.nullable {
            format!("Option<{}>", self.value_type())
        } else {
            self.value_type().to_owned()
        }
    }

    /// Whether a typed id can wrap this column's type: the type must be
    /// `Eq`, `Ord`, `Hash`, and `Display`.
    pub fn supports_typed_id(&self) -> bool {
        matches!(
            self.sqlx_type.as_str(),
            "i16"
                | "i32"
                | "i64"
                | "bool"
                | "String"
                | "uuid::Uuid"
                | "rust_decimal::Decimal"
                | "chrono::NaiveDate"
                | "chrono::NaiveTime"
                | "chrono::NaiveDateTime"
                | "chrono::DateTime<chrono::Utc>"
        )
    }

    /// Whether the column's value type is `Copy`.
    pub fn is_copy(&self) -> bool {
        self.supports_typed_id() && self.sqlx_type != "String"
    }
}

#[derive(Debug, Clone)]
pub struct ForeignKey {
    pub column: String,
    /// The referenced table's schema qualifier as written; `None` when the
    /// reference is unqualified.
    pub referenced_schema: Option<String>,
    pub referenced_table: String,
    pub referenced_column: String,
}

/// Columns no two rows share, from a unique constraint or a unique index on
/// plain columns. Input to unique-key lookups; nothing reads it yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniqueKey {
    /// Key columns in declaration order.
    pub columns: Vec<String>,
    /// The `WHERE` condition of a partial unique index, as SQL text; `None`
    /// for a constraint or a full index.
    pub predicate: Option<String>,
}

impl UniqueKey {
    /// A unique key with no partial-index condition.
    pub fn new(columns: Vec<String>) -> Self {
        Self {
            columns,
            predicate: None,
        }
    }
}
