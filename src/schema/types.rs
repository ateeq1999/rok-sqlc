use heck::ToUpperCamelCase;

#[derive(Debug, Clone)]
pub struct Schema {
    pub tables: Vec<Table>,
    /// Enum types in definition order, generated into the shared `types` module.
    pub enums: Vec<EnumType>,
}

/// The shared generated module holding enum types. A column of an enum type
/// has the Rust type `types::{Name}`, and every generated file mentioning one
/// imports `types` along its path to the `db` module.
pub const TYPES_MODULE: &str = "types";

/// A `CREATE TYPE … AS ENUM` type, after every `ALTER TYPE` applied to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumType {
    /// The schema qualifier as written (`app` in `app.status`); `None` when
    /// unqualified or qualified with `public`.
    pub schema: Option<String>,
    pub sql_name: String,
    /// The Rust enum name in the `types` module.
    pub rust_name: String,
    /// Variants in the database's sort order.
    pub variants: Vec<EnumVariant>,
    /// For a value-list column's enum, the column it restricts as written in
    /// DDL (`catalog.products.product_type`); such an enum encodes as text.
    /// `None` for a database enum type.
    pub column: Option<String>,
}

impl EnumType {
    /// The type name as written in DDL, e.g. `app.status`.
    pub fn display_name(&self) -> String {
        match &self.schema {
            Some(schema) => format!("{schema}.{}", self.sql_name),
            None => self.sql_name.clone(),
        }
    }

    /// The Rust type of a column of this enum, e.g. `types::Status`.
    pub fn rust_path(&self) -> String {
        format!("{TYPES_MODULE}::{}", self.rust_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumVariant {
    /// The exact database label.
    pub label: String,
    pub rust_name: String,
}

/// Whether a Rust type mentions a generated enum (`types::X`, possibly inside
/// `Option<…>` or `Vec<…>`), so its file must import the `types` module.
pub fn mentions_generated_type(rust_type: &str) -> bool {
    let needle = format!("{TYPES_MODULE}::");
    rust_type.match_indices(&needle).any(|(index, _)| {
        !rust_type[..index]
            .ends_with(|character: char| character.is_alphanumeric() || "_:".contains(character))
    })
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
    /// The version column (ADR 0010), as its SQL name: set only when the
    /// version convention is configured and this table has the column. The
    /// parser leaves it `None`; the generator applies the conventions.
    pub version_column: Option<String>,
    /// The soft-delete column (ADR 0010), when the soft-delete convention is
    /// configured and this table has that column. The parser leaves it
    /// `None`; the generator applies the conventions.
    pub soft_delete_column: Option<String>,
}

impl Table {
    /// The table name as a human reads it in DDL, e.g. `catalog.products`.
    pub fn display_name(&self) -> String {
        match &self.schema {
            Some(schema) => format!("{schema}.{}", self.sql_name),
            None => self.sql_name.clone(),
        }
    }

    /// The table's unique keys as column lists: the primary key first, then
    /// each unique constraint or index that [counts](UniqueKey::counts), in
    /// declaration order. A key over the same columns as an earlier one (in
    /// any order) is left out.
    pub fn unique_key_columns(&self, soft_delete_column: Option<&str>) -> Vec<Vec<String>> {
        let mut keys: Vec<Vec<String>> = Vec::new();
        if !self.primary_key.is_empty() {
            keys.push(self.primary_key.clone());
        }
        let declared = self
            .unique_keys
            .iter()
            .filter(|key| key.counts(soft_delete_column));
        for key in declared {
            let same = |other: &Vec<String>| {
                other.len() == key.columns.len()
                    && key.columns.iter().all(|column| other.contains(column))
            };
            if !keys.iter().any(same) {
                keys.push(key.columns.clone());
            }
        }
        keys
    }

    /// Primary key columns in key declaration order.
    pub fn primary_key_columns(&self) -> Vec<&Column> {
        self.primary_key
            .iter()
            .filter_map(|key| self.columns.iter().find(|column| column.sql_name == *key))
            .collect()
    }

    /// Whether `column` is this table's soft-delete column.
    pub fn is_soft_delete_column(&self, column: &Column) -> bool {
        self.soft_delete_column.as_deref() == Some(column.sql_name.as_str())
    }

    /// Columns a caller may supply on insert. The soft-delete column is left
    /// out: only `delete` and `restore` set it.
    pub fn insert_columns(&self) -> Vec<&Column> {
        self.columns
            .iter()
            .filter(|column| column.is_insertable() && !self.is_soft_delete_column(column))
            .collect()
    }

    /// Columns a caller may change on update. The soft-delete column is left
    /// out: only `delete` and `restore` set it.
    pub fn update_columns(&self) -> Vec<&Column> {
        self.columns
            .iter()
            .filter(|column| column.is_updatable() && !self.is_soft_delete_column(column))
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
    /// The values a `CHECK (col IN ('a', ...))` restricts this text column to,
    /// deduplicated in list order. The parser turns such a column into a
    /// value-list enum; `None` for other columns.
    pub value_list: Option<Vec<String>>,
    /// The typed id the generated code uses in place of `sqlx_type`: the
    /// table's own id for a single-column primary key, or the referenced
    /// table's id for a foreign key to it. The parser leaves it `None`; the
    /// generator fills it in for the tables it generates.
    pub typed_id: Option<TypedId>,
    /// A managed column (ADR 0010): the database maintains it, so it is
    /// never inserted or updated by generated code but stays in rows. The
    /// parser leaves it `false`; the generator applies the conventions.
    pub is_managed: bool,
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
    /// Booleans, UUIDs, and enums: equality and sets.
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
            enum_type if enum_type.starts_with("types::") => Some(FilterKind::Equality),
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

    /// The caller may supply this column on insert: it is neither generated
    /// nor managed.
    pub fn is_insertable(&self) -> bool {
        !self.is_generated && !self.is_managed
    }

    /// The caller may omit this column on insert so the database default applies.
    pub fn is_omittable_on_insert(&self) -> bool {
        self.is_insertable() && self.has_default
    }

    /// The caller may change this column on update: it is neither generated,
    /// managed, nor part of the primary key.
    pub fn is_updatable(&self) -> bool {
        !self.is_generated && !self.is_managed && !self.is_primary_key
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
/// plain columns, as declared. Whether a partial index makes a unique key is
/// decided by [`UniqueKey::counts`].
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

    /// Whether the declaration makes a unique key: a constraint or a full
    /// index always does; a partial index only when its condition is exactly
    /// `<soft_delete_column> IS NULL`, so the key holds across live rows.
    pub fn counts(&self, soft_delete_column: Option<&str>) -> bool {
        let Some(predicate) = &self.predicate else {
            return true;
        };
        let Some(column) = soft_delete_column else {
            return false;
        };
        let mut condition = predicate.trim();
        while let Some(inner) = condition
            .strip_prefix('(')
            .and_then(|rest| rest.strip_suffix(')'))
        {
            condition = inner.trim();
        }
        let Some(subject) = condition
            .strip_suffix("IS NULL")
            .or_else(|| condition.strip_suffix("is null"))
        else {
            return false;
        };
        let subject = subject.trim();
        subject == column || subject == format!("\"{}\"", column.replace('"', "\"\""))
    }
}

#[cfg(test)]
mod tests {
    use super::UniqueKey;

    fn partial(predicate: &str) -> UniqueKey {
        UniqueKey {
            columns: vec!["sku".to_owned()],
            predicate: Some(predicate.to_owned()),
        }
    }

    #[test]
    fn a_partial_index_counts_only_on_exactly_the_soft_delete_condition() {
        let soft_delete = Some("deleted_at");
        assert!(UniqueKey::new(vec!["sku".to_owned()]).counts(None));
        assert!(partial("deleted_at IS NULL").counts(soft_delete));
        assert!(partial("(deleted_at IS NULL)").counts(soft_delete));
        assert!(!partial("deleted_at IS NULL").counts(None));
        assert!(!partial("archived_at IS NULL").counts(soft_delete));
        assert!(!partial("deleted_at IS NOT NULL").counts(soft_delete));
        assert!(!partial("deleted_at IS NULL AND sku <> ''").counts(soft_delete));
    }
}
